// ZerolibPatch -- give bflat's zerolib the runtime helpers its arm64 code
// generator asks for.
//
// On arm64, RyuJIT/ILC emits explicit checks around integer division (the
// `sdiv` instruction does not trap like x86 `idiv`): divide-by-zero calls
// ThrowHelpers.ThrowDivideByZeroException, and INT_MIN / -1 calls
// ThrowHelpers.ThrowOverflowException. zerolib (bflat --stdlib:zero) ships the
// first but not the second, so every arm64 program with a signed division
// fails to compile ("Expected method 'ThrowOverflowException' not found on type
// '[zerolib]Internal.Runtime.CompilerHelpers.ThrowHelpers'").
//
// It also renames zerolib's RhpStelemRef, RhpNewFast and RhpNewArray exports
// (see below) on every architecture: the startup shim implements the real
// covariant array store and allocates objects on its collected heap.
//
// It also adds System.IDisposable (`using`) when zerolib lacks it.
//
// This tool reads a zerolib.dll, adds each missing helper to that class with
// the IL body of an existing one (zerolib has no exception machinery: every
// throw helper ends the program), and writes a patched copy. The original is
// never modified.
//
// Usage: ZerolibPatch <in zerolib.dll> <out zerolib.dll>

using Mono.Cecil;
using Mono.Cecil.Cil;

if (args.Length != 2)
{
    Console.Error.WriteLine("usage: ZerolibPatch <in zerolib.dll> <out zerolib.dll>");
    return 2;
}

string[] wanted = ["ThrowOverflowException"];
const string template = "ThrowDivideByZeroException";

var module = ModuleDefinition.ReadModule(args[0], new ReaderParameters { ReadSymbols = false });
var helpers = module.GetType("Internal.Runtime.CompilerHelpers.ThrowHelpers");
if (helpers is null)
{
    Console.Error.WriteLine("ThrowHelpers not found");
    return 1;
}
var source = helpers.Methods.FirstOrDefault(m => m.Name == template && m.Parameters.Count == 0);
if (source is null)
{
    Console.Error.WriteLine($"{template} not found (cannot clone a body)");
    return 1;
}

int added = 0;
foreach (var name in wanted)
{
    if (helpers.Methods.Any(m => m.Name == name))
    {
        Console.WriteLine($"{name}: already present");
        continue;
    }
    var m = new MethodDefinition(name, source.Attributes, source.ReturnType);
    m.Body.InitLocals = source.Body.InitLocals;
    foreach (var v in source.Body.Variables)
        m.Body.Variables.Add(new VariableDefinition(v.VariableType));
    // Copy the IL instruction by instruction (Cecil instructions belong to one
    // body); branch targets are remapped to the copies.
    var il = m.Body.GetILProcessor();
    var map = new Dictionary<Instruction, Instruction>();
    var copies = new List<Instruction>();
    foreach (var ins in source.Body.Instructions)
    {
        var copy = Instruction.Create(OpCodes.Nop);
        copy.OpCode = ins.OpCode;
        copy.Operand = ins.Operand;
        map[ins] = copy;
        copies.Add(copy);
    }
    foreach (var copy in copies)
    {
        if (copy.Operand is Instruction target)
            copy.Operand = map[target];
        else if (copy.Operand is Instruction[] targets)
            copy.Operand = targets.Select(t => map[t]).ToArray();
        else if (copy.Operand is VariableDefinition v)
            copy.Operand = m.Body.Variables[v.Index];
        il.Append(copy);
    }
    foreach (var attr in source.CustomAttributes)
        m.CustomAttributes.Add(attr);
    helpers.Methods.Add(m);
    Console.WriteLine($"{name}: added (body of {template})");
    added++;
}

// `using (x) { }` needs System.IDisposable, which the C# compiler only looks
// for in the core library: add the interface when zerolib lacks it.
int addedTypes = 0;
if (module.GetType("System.IDisposable") is null)
{
    var disposable = new TypeDefinition("System", "IDisposable",
        TypeAttributes.Public | TypeAttributes.Interface | TypeAttributes.Abstract);
    var dispose = new MethodDefinition("Dispose",
        MethodAttributes.Public | MethodAttributes.Abstract | MethodAttributes.Virtual |
        MethodAttributes.HideBySig | MethodAttributes.NewSlot,
        // `void` as decoded from an existing signature (ELEMENT_TYPE_VOID):
        // module.TypeSystem.Void would reference mscorlib (zerolib is not
        // recognised as a core library), and the TypeDefinition System.Void
        // would be written as a class type, which the C# compiler rejects.
        source.ReturnType);
    disposable.Methods.Add(dispose);
    module.Types.Add(disposable);
    addedTypes++;
    Console.WriteLine("System.IDisposable: added");
}

// ---- exceptions (patch v8) ----------------------------------------------------
// The base exception types live in zerolib, so the runtime's own checks can
// throw them: ThrowHelpers (array index, division, overflow) now build the
// exception and `throw` it instead of FailFast, and BzThrow exports throw
// helpers the startup shim jumps to (invalid cast, array store, and the
// kernel's FAULT_HANDLER redirect of a null dereference / #DE). The other
// exception types stay in userland/hello-csharp/bzexc.cs.
var voidType = source.ReturnType;
// `string` as decoded from an existing signature (ELEMENT_TYPE_STRING, not a
// class token: the C# compiler cannot use a String encoded as a class)
var stringType = module.Types.SelectMany(t => t.Methods).SelectMany(m => m.Parameters)
    .Select(p => p.ParameterType).FirstOrDefault(t => t.MetadataType == MetadataType.String);
var objectType = module.GetType("System.Object");
var objectCtor = objectType?.Methods.FirstOrDefault(m => m.IsConstructor && !m.IsStatic && m.Parameters.Count == 0);
int addedExceptions = 0;
var exTypes = new Dictionary<string, TypeDefinition>();
const MethodAttributes ctorAttrs = MethodAttributes.Public | MethodAttributes.HideBySig | MethodAttributes.SpecialName | MethodAttributes.RTSpecialName;
MethodDefinition Ctor(TypeDefinition t, params TypeReference[] ps)
{
    var c = new MethodDefinition(".ctor", ctorAttrs, voidType);
    foreach (var p in ps) c.Parameters.Add(new ParameterDefinition(p));
    t.Methods.Add(c);
    return c;
}
if (module.GetType("System.Exception") is null && stringType is not null && objectCtor is not null)
{
    // System.Exception: Message (first field: the shim reads it at obj+8), InnerException
    var ex = new TypeDefinition("System", "Exception", TypeAttributes.Public | TypeAttributes.Class | TypeAttributes.BeforeFieldInit, objectType);
    module.Types.Add(ex);
    var msg = new FieldDefinition("Message", FieldAttributes.Public, stringType);
    var inner = new FieldDefinition("InnerException", FieldAttributes.Public, ex);
    ex.Fields.Add(msg);
    ex.Fields.Add(inner);
    var il = Ctor(ex).Body.GetILProcessor();
    il.Emit(OpCodes.Ldarg_0); il.Emit(OpCodes.Call, objectCtor);
    il.Emit(OpCodes.Ldarg_0); il.Emit(OpCodes.Ldstr, "Exception of type System.Exception was thrown."); il.Emit(OpCodes.Stfld, msg);
    il.Emit(OpCodes.Ret);
    il = Ctor(ex, stringType).Body.GetILProcessor();
    il.Emit(OpCodes.Ldarg_0); il.Emit(OpCodes.Call, objectCtor);
    il.Emit(OpCodes.Ldarg_0); il.Emit(OpCodes.Ldarg_1); il.Emit(OpCodes.Stfld, msg);
    il.Emit(OpCodes.Ret);
    il = Ctor(ex, stringType, ex).Body.GetILProcessor();
    il.Emit(OpCodes.Ldarg_0); il.Emit(OpCodes.Call, objectCtor);
    il.Emit(OpCodes.Ldarg_0); il.Emit(OpCodes.Ldarg_1); il.Emit(OpCodes.Stfld, msg);
    il.Emit(OpCodes.Ldarg_0); il.Emit(OpCodes.Ldarg_2); il.Emit(OpCodes.Stfld, inner);
    il.Emit(OpCodes.Ret);
    exTypes["Exception"] = ex;
    addedExceptions++;
    void Derived(string name, string baseName, string message, bool withInner)
    {
        var b = exTypes[baseName];
        var t = new TypeDefinition("System", name, TypeAttributes.Public | TypeAttributes.Class | TypeAttributes.BeforeFieldInit, b);
        module.Types.Add(t);
        var baseStr = b.Methods.First(m => m.IsConstructor && m.Parameters.Count == 1);
        var p = Ctor(t).Body.GetILProcessor();
        p.Emit(OpCodes.Ldarg_0); p.Emit(OpCodes.Ldstr, message); p.Emit(OpCodes.Call, baseStr); p.Emit(OpCodes.Ret);
        p = Ctor(t, stringType).Body.GetILProcessor();
        p.Emit(OpCodes.Ldarg_0); p.Emit(OpCodes.Ldarg_1); p.Emit(OpCodes.Call, baseStr); p.Emit(OpCodes.Ret);
        if (withInner)
        {
            var baseInner = b.Methods.First(m => m.IsConstructor && m.Parameters.Count == 2);
            p = Ctor(t, stringType, exTypes["Exception"]).Body.GetILProcessor();
            p.Emit(OpCodes.Ldarg_0); p.Emit(OpCodes.Ldarg_1); p.Emit(OpCodes.Ldarg_2); p.Emit(OpCodes.Call, baseInner); p.Emit(OpCodes.Ret);
        }
        exTypes[name] = t;
        addedExceptions++;
    }
    Derived("SystemException", "Exception", "System error.", true);
    Derived("IndexOutOfRangeException", "SystemException", "Index was outside the bounds of the array.", false);
    Derived("NullReferenceException", "SystemException", "Object reference not set to an instance of an object.", false);
    Derived("InvalidCastException", "SystemException", "Specified cast is not valid.", false);
    Derived("OverflowException", "SystemException", "Arithmetic operation resulted in an overflow.", false);
    Derived("DivideByZeroException", "SystemException", "Attempted to divide by zero.", false);
    Derived("ArrayTypeMismatchException", "SystemException", "Attempted to access an element as a type incompatible with the array.", false);
    Console.WriteLine($"exceptions: {addedExceptions} type(s) added");
}

void ThrowBody(MethodDefinition m, TypeDefinition exType)
{
    var ctor = exType.Methods.First(c => c.IsConstructor && c.Parameters.Count == 0);
    m.Body.Instructions.Clear();
    m.Body.Variables.Clear();
    m.Body.ExceptionHandlers.Clear();
    var il = m.Body.GetILProcessor();
    il.Emit(OpCodes.Newobj, ctor);
    il.Emit(OpCodes.Throw);
}

int throwing = 0;
if (exTypes.Count > 0)
{
    foreach (var (helper, exName) in new[] {
        ("ThrowIndexOutOfRangeException", "IndexOutOfRangeException"),
        ("ThrowDivideByZeroException", "DivideByZeroException"),
        ("ThrowOverflowException", "OverflowException") })
    {
        var m = helpers.Methods.FirstOrDefault(x => x.Name == helper && x.Parameters.Count == 0);
        if (m is null) continue;
        ThrowBody(m, exTypes[exName]);
        throwing++;
    }
    var exportAttr = module.GetType("System.Runtime.RuntimeExportAttribute");
    var exportCtor = exportAttr?.Methods.FirstOrDefault(c => c.IsConstructor && c.Parameters.Count == 1);
    if (exportCtor is not null)
    {
        var bz = new TypeDefinition("Internal.Runtime.CompilerHelpers", "BzThrow",
            TypeAttributes.NotPublic | TypeAttributes.Class | TypeAttributes.Abstract | TypeAttributes.Sealed, objectType);
        module.Types.Add(bz);
        foreach (var (export, exName) in new[] {
            ("bz_throw_null_reference", "NullReferenceException"),
            ("bz_throw_divide_by_zero", "DivideByZeroException"),
            ("bz_throw_index_out_of_range", "IndexOutOfRangeException"),
            ("bz_throw_invalid_cast", "InvalidCastException"),
            ("bz_throw_array_type_mismatch", "ArrayTypeMismatchException"),
            ("bz_throw_overflow", "OverflowException") })
        {
            var m = new MethodDefinition(export.Replace("bz_throw_", "Throw_"),
                MethodAttributes.Public | MethodAttributes.Static | MethodAttributes.HideBySig, voidType);
            var attr = new CustomAttribute(exportCtor);
            attr.ConstructorArguments.Add(new CustomAttributeArgument(stringType, export));
            m.CustomAttributes.Add(attr);
            bz.Methods.Add(m);
            ThrowBody(m, exTypes[exName]);
            throwing++;
        }
    }
    Console.WriteLine($"throw helpers: {throwing} now throw managed exceptions");
}

// The shim also allocates every managed object (RhpNewFast / RhpNewArray) on
// the heap its garbage collector manages (zerolib's versions call malloc).
string[] shimExports = ["RhpStelemRef", "RhpNewFast", "RhpNewArray"];
// zerolib's RhpStelemRef (`array[i] = obj` on a reference-type array)
// fail-fasts unless obj's type is exactly the element type, so `object[]`,
// base-class and interface arrays could not hold anything else. The startup
// shim (bzstart.rs) provides a real one (base chain + interface map); rename
// zerolib's export so the shim's is the only `RhpStelemRef`.
int renamed = 0;
foreach (var type in module.Types)
{
    foreach (var method in type.Methods)
    {
        foreach (var attr in method.CustomAttributes)
        {
            if (attr.AttributeType.Name != "RuntimeExportAttribute" || attr.ConstructorArguments.Count != 1)
                continue;
            var arg = attr.ConstructorArguments[0];
            if (arg.Value is not string name || !shimExports.Contains(name))
                continue;
            attr.ConstructorArguments[0] = new CustomAttributeArgument(arg.Type, name + "_zerolib");
            renamed++;
        }
    }
}
Console.WriteLine($"shim exports: {renamed} of {shimExports.Length} renamed to <name>_zerolib");

// Cecil can add an unused mscorlib reference when it looks for the core
// library; the C# compiler takes the assembly without references as corlib,
// so a stray one makes it report "System.Object is not defined".
var usedScopes = module.GetTypeReferences().Select(t => t.Scope).OfType<AssemblyNameReference>().Select(r => r.Name).ToHashSet();
foreach (var r in module.AssemblyReferences.Where(r => !usedScopes.Contains(r.Name)).ToList())
    module.AssemblyReferences.Remove(r);
module.Write(args[1]);
Console.WriteLine($"wrote {args[1]} ({added} helper(s) added, {addedTypes} type(s) added, {renamed} export(s) renamed)");
return 0;

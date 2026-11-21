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

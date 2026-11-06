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
// It also renames zerolib's RhpStelemRef export (see below) on every
// architecture: the startup shim implements the real covariant array store.
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
            if (arg.Value as string != "RhpStelemRef")
                continue;
            attr.ConstructorArguments[0] = new CustomAttributeArgument(arg.Type, "RhpStelemRef_zerolib");
            renamed++;
        }
    }
}
Console.WriteLine($"RhpStelemRef: {(renamed > 0 ? "export renamed to RhpStelemRef_zerolib" : "not found")}");

module.Write(args[1]);
Console.WriteLine($"wrote {args[1]} ({added} helper(s) added, {renamed} export(s) renamed)");
return 0;

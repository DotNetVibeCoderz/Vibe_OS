// Package signatures: `bz keygen` makes an Ed25519 key (~/.bz/signing.key),
// `bz pack --sign` appends "BZSG" + the public key + an Ed25519 signature over
// the whole archive (kernel/bzkernel/src/bzpkg.rs verifies it), and `bz
// inspect` checks it. On the device: `bz trust add <public key> <name>`, and
// `bz trust require on` to refuse anything else.

using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Crypto.Signers;

static class Signing
{
    static readonly byte[] Magic = "BZSG"u8.ToArray();
    public const int TrailerLength = 4 + 32 + 64;

    public static string KeyPath => Environment.GetEnvironmentVariable("BZ_SIGNING_KEY") ?? Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".bz", "signing.key");

    static int Fail(string m) { Console.Error.WriteLine("bz: " + m); return 1; }

    /// <summary>`bz keygen [--force]`: a new signing key; prints the public key.</summary>
    public static int Keygen(bool force)
    {
        if (File.Exists(KeyPath) && !force)
        {
            var existing = Load();
            Console.WriteLine($"a signing key already exists ({KeyPath}); --force replaces it");
            Console.WriteLine("public key: " + Hex(existing!.GeneratePublicKey().GetEncoded()));
            return 1;
        }
        var seed = System.Security.Cryptography.RandomNumberGenerator.GetBytes(32);
        Directory.CreateDirectory(Path.GetDirectoryName(KeyPath)!);
        File.WriteAllText(KeyPath, Hex(seed) + "\n");
        var pub = new Ed25519PrivateKeyParameters(seed, 0).GeneratePublicKey().GetEncoded();
        Console.WriteLine($"wrote {KeyPath} (keep it private)");
        Console.WriteLine("public key: " + Hex(pub));
        Console.WriteLine($"on the device: bz trust add {Hex(pub)} <your-name>");
        return 0;
    }

    static Ed25519PrivateKeyParameters? Load()
    {
        if (!File.Exists(KeyPath)) return null;
        var hex = File.ReadAllText(KeyPath).Trim();
        return hex.Length == 64 ? new Ed25519PrivateKeyParameters(Convert.FromHexString(hex), 0) : null;
    }

    /// <summary>The archive followed by its signature trailer, or null without a key.</summary>
    public static byte[]? Sign(byte[] archive)
    {
        var key = Load();
        if (key is null) return null;
        var signer = new Ed25519Signer();
        signer.Init(true, key);
        signer.BlockUpdate(archive, 0, archive.Length);
        var sig = signer.GenerateSignature();
        return archive.Concat(Magic).Concat(key.GeneratePublicKey().GetEncoded()).Concat(sig).ToArray();
    }

    /// <summary>Split a signed archive: (archive, public key, signature valid);
    /// null key = unsigned.</summary>
    public static (byte[] Archive, byte[]? Key, bool Valid) Split(byte[] b)
    {
        int n = b.Length;
        if (n <= TrailerLength + 44 || !b.AsSpan(n - TrailerLength, 4).SequenceEqual(Magic))
            return (b, null, false);
        var body = b[..(n - TrailerLength)];
        var key = b[(n - 96)..(n - 64)];
        var sig = b[(n - 64)..];
        var verifier = new Ed25519Signer();
        verifier.Init(false, new Ed25519PublicKeyParameters(key, 0));
        verifier.BlockUpdate(body, 0, body.Length);
        return (body, key, verifier.VerifySignature(sig));
    }

    public static string Hex(byte[] b) => Convert.ToHexString(b).ToLowerInvariant();
}

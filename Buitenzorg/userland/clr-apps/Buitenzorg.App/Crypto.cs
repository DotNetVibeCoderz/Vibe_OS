// Buitenzorg.Security.Cryptography: hashes, HMAC, AES-GCM, ECDSA P-256 and RSA
// signature checks for managed apps. RustBCL has no System.Security.Cryptography,
// so on the device every call goes to clrhost (cryptoapi.rs, the RustCrypto
// crates the TLS client links); off the device the same API runs on .NET's
// System.Security.Cryptography, so `dotnet X.dll --selftest` gives the same answers.
using System;
using System.Runtime.InteropServices;

namespace Buitenzorg.Security.Cryptography;

public enum HashAlgorithm { Sha256 = 0, Sha384 = 1, Sha512 = 2 }

internal static class CryptoNative
{
    [DllImport("buitenzorg")] internal static extern int CryptoHash(int alg, byte[] data, byte[] output);
    [DllImport("buitenzorg")] internal static extern int CryptoHmac(int alg, byte[] key, byte[] data, byte[] output);
    [DllImport("buitenzorg")] internal static extern int CryptoAesGcm(int encrypt, byte[] key, byte[] nonce, byte[] aad, byte[] input, byte[] output);
    [DllImport("buitenzorg")] internal static extern int CryptoEcdsa(int op, byte[] a, byte[] b, byte[] c, byte[] output);
    [DllImport("buitenzorg")] internal static extern int CryptoRsaVerify(byte[] modulus, byte[] exponent, byte[] data, byte[] signature);
    [DllImport("buitenzorg")] internal static extern int CryptoRandom(byte[] output);
    [DllImport("buitenzorg")] internal static extern int CryptoRootCount();
    [DllImport("buitenzorg")] internal static extern string CryptoRootName(int index);

    static int _device; // 0 unknown, 1 device, 2 host
    /// <summary>True when the natives exist (on Buitenzorg).</summary>
    internal static bool OnDevice
    {
        get
        {
            if (_device == 0)
            {
                try { CryptoRootCount(); _device = 1; }
                catch (Exception e) when (e is DllNotFoundException || e is EntryPointNotFoundException) { _device = 2; }
            }
            return _device == 1;
        }
    }

    internal static byte[] Trim(byte[] buf, int n)
    {
        if (n < 0) throw new InvalidOperationException("crypto: operation failed");
        if (n == buf.Length) return buf;
        var r = new byte[n];
        for (int i = 0; i < n; i++) r[i] = buf[i];
        return r;
    }

    internal static int HashSize(HashAlgorithm a) => a == HashAlgorithm.Sha256 ? 32 : a == HashAlgorithm.Sha384 ? 48 : 64;
}

public static class Hash
{
    /// <summary>SHA-256 / SHA-384 / SHA-512 of <paramref name="data"/>.</summary>
    public static byte[] Compute(HashAlgorithm alg, byte[] data)
    {
        if (CryptoNative.OnDevice)
        {
            var o = new byte[CryptoNative.HashSize(alg)];
            return CryptoNative.Trim(o, CryptoNative.CryptoHash((int)alg, data, o));
        }
        return Host.Hash(alg, data);
    }

    public static byte[] Sha256(byte[] data) => Compute(HashAlgorithm.Sha256, data);
    public static byte[] Sha256(string text) => Compute(HashAlgorithm.Sha256, System.Text.Encoding.UTF8.GetBytes(text));

    /// <summary>HMAC of <paramref name="data"/> under <paramref name="key"/>.</summary>
    public static byte[] Hmac(HashAlgorithm alg, byte[] key, byte[] data)
    {
        if (CryptoNative.OnDevice)
        {
            var o = new byte[CryptoNative.HashSize(alg)];
            return CryptoNative.Trim(o, CryptoNative.CryptoHmac((int)alg, key, data, o));
        }
        return Host.Hmac(alg, key, data);
    }

    /// <summary>Lower-case hex of some bytes.</summary>
    public static string ToHex(byte[] b)
    {
        const string d = "0123456789abcdef";
        var c = new char[b.Length * 2];
        for (int i = 0; i < b.Length; i++) { c[2 * i] = d[b[i] >> 4]; c[2 * i + 1] = d[b[i] & 15]; }
        return new string(c);
    }

    public static byte[] FromHex(string s)
    {
        var r = new byte[s.Length / 2];
        for (int i = 0; i < r.Length; i++) r[i] = (byte)(Nib(s[2 * i]) << 4 | Nib(s[2 * i + 1]));
        return r;
    }

    static int Nib(char c) => c >= 'a' ? c - 'a' + 10 : c >= 'A' ? c - 'A' + 10 : c - '0';
}

/// <summary>Cryptographically strong random bytes (the kernel's RANDOM syscall).</summary>
public static class SecureRandom
{
    public static byte[] Bytes(int n)
    {
        var o = new byte[n];
        if (CryptoNative.OnDevice) return CryptoNative.Trim(o, CryptoNative.CryptoRandom(o));
        System.Security.Cryptography.RandomNumberGenerator.Fill(o);
        return o;
    }
}

/// <summary>AES-GCM with a 16- or 32-byte key and a 12-byte nonce; the ciphertext carries its 16-byte tag at the end.</summary>
public sealed class AesGcm
{
    readonly byte[] _key;
    public AesGcm(byte[] key)
    {
        if (key.Length != 16 && key.Length != 32) throw new ArgumentException("AES-GCM key must be 16 or 32 bytes");
        _key = key;
    }

    public byte[] Encrypt(byte[] nonce, byte[] plaintext, byte[]? aad = null)
    {
        aad ??= new byte[0];
        if (CryptoNative.OnDevice)
        {
            var o = new byte[plaintext.Length + 16];
            return CryptoNative.Trim(o, CryptoNative.CryptoAesGcm(1, _key, nonce, aad, plaintext, o));
        }
        return Host.Gcm(true, _key, nonce, aad, plaintext);
    }

    /// <summary>The plaintext, or null when the tag does not match (wrong key, nonce, data or aad).</summary>
    public byte[]? Decrypt(byte[] nonce, byte[] ciphertext, byte[]? aad = null)
    {
        aad ??= new byte[0];
        if (ciphertext.Length < 16) return null;
        if (CryptoNative.OnDevice)
        {
            var o = new byte[ciphertext.Length - 16];
            int n = CryptoNative.CryptoAesGcm(0, _key, nonce, aad, ciphertext, o);
            return n < 0 ? null : CryptoNative.Trim(o, n);
        }
        try { return Host.Gcm(false, _key, nonce, aad, ciphertext); }
        catch (System.Security.Cryptography.CryptographicException) { return null; }
    }
}

/// <summary>An ECDSA P-256 key pair (SHA-256): 32-byte private key, 65-byte uncompressed public key, 64-byte r || s signatures.</summary>
public sealed class EcdsaP256
{
    public byte[] PrivateKey { get; }
    public byte[] PublicKey { get; }
    EcdsaP256(byte[] priv, byte[] pub) { PrivateKey = priv; PublicKey = pub; }

    public static EcdsaP256 Generate()
    {
        if (CryptoNative.OnDevice)
        {
            var o = new byte[97];
            CryptoNative.Trim(o, CryptoNative.CryptoEcdsa(0, new byte[0], new byte[0], new byte[0], o));
            var priv = new byte[32];
            var pub = new byte[65];
            Array.Copy(o, 0, priv, 0, 32);
            Array.Copy(o, 32, pub, 0, 65);
            return new EcdsaP256(priv, pub);
        }
        var (p, q) = Host.EcGenerate();
        return new EcdsaP256(p, q);
    }

    public byte[] Sign(byte[] data)
    {
        if (CryptoNative.OnDevice)
        {
            var o = new byte[64];
            return CryptoNative.Trim(o, CryptoNative.CryptoEcdsa(1, PrivateKey, data, new byte[0], o));
        }
        return Host.EcSign(PrivateKey, PublicKey, data);
    }

    public static bool Verify(byte[] publicKey, byte[] data, byte[] signature)
    {
        if (CryptoNative.OnDevice) return CryptoNative.CryptoEcdsa(2, publicKey, data, signature, new byte[0]) == 1;
        return Host.EcVerify(publicKey, data, signature);
    }
}

public static class Rsa
{
    /// <summary>Check an RSA PKCS#1 v1.5 SHA-256 signature against a public key given as modulus and exponent (big-endian).</summary>
    public static bool VerifyPkcs1Sha256(byte[] modulus, byte[] exponent, byte[] data, byte[] signature)
    {
        if (CryptoNative.OnDevice) return CryptoNative.CryptoRsaVerify(modulus, exponent, data, signature) == 1;
        return Host.RsaVerify(modulus, exponent, data, signature);
    }
}

/// <summary>The root certificate authorities the HTTPS client trusts (Mozilla's list).</summary>
public static class RootCertificates
{
    public static int Count => CryptoNative.OnDevice ? CryptoNative.CryptoRootCount() : HostRoots.Length;
    public static string Name(int i) => CryptoNative.OnDevice ? CryptoNative.CryptoRootName(i) : (i >= 0 && i < HostRoots.Length ? HostRoots[i] : "");
    // off the device: a few well-known names (the list itself lives in the host)
    static readonly string[] HostRoots = { "ISRG Root X1", "ISRG Root X2", "DigiCert Global Root G2", "GlobalSign Root CA" };
}

/// <summary>Off the device: .NET's implementations.</summary>
static class Host
{
    internal static byte[] Hash(HashAlgorithm a, byte[] d) => a switch
    {
        HashAlgorithm.Sha256 => System.Security.Cryptography.SHA256.HashData(d),
        HashAlgorithm.Sha384 => System.Security.Cryptography.SHA384.HashData(d),
        _ => System.Security.Cryptography.SHA512.HashData(d),
    };

    internal static byte[] Hmac(HashAlgorithm a, byte[] k, byte[] d) => a switch
    {
        HashAlgorithm.Sha256 => System.Security.Cryptography.HMACSHA256.HashData(k, d),
        HashAlgorithm.Sha384 => System.Security.Cryptography.HMACSHA384.HashData(k, d),
        _ => System.Security.Cryptography.HMACSHA512.HashData(k, d),
    };

    internal static byte[] Gcm(bool enc, byte[] key, byte[] nonce, byte[] aad, byte[] input)
    {
        using var g = new System.Security.Cryptography.AesGcm(key, 16);
        if (enc)
        {
            var c = new byte[input.Length];
            var tag = new byte[16];
            g.Encrypt(nonce, input, c, tag, aad);
            var r = new byte[c.Length + 16];
            c.CopyTo(r, 0);
            tag.CopyTo(r, c.Length);
            return r;
        }
        var ct = input.AsSpan(0, input.Length - 16);
        var t = input.AsSpan(input.Length - 16);
        var p = new byte[ct.Length];
        g.Decrypt(nonce, ct, t, p, aad);
        return p;
    }

    static System.Security.Cryptography.ECDsa Ec(byte[]? priv, byte[] pub)
    {
        var x = new byte[32];
        var y = new byte[32];
        Array.Copy(pub, 1, x, 0, 32);
        Array.Copy(pub, 33, y, 0, 32);
        var p = new System.Security.Cryptography.ECParameters
        {
            Curve = System.Security.Cryptography.ECCurve.NamedCurves.nistP256,
            Q = new System.Security.Cryptography.ECPoint { X = x, Y = y },
            D = priv,
        };
        return System.Security.Cryptography.ECDsa.Create(p);
    }

    internal static (byte[], byte[]) EcGenerate()
    {
        using var e = System.Security.Cryptography.ECDsa.Create(System.Security.Cryptography.ECCurve.NamedCurves.nistP256);
        var p = e.ExportParameters(true);
        var pub = new byte[65];
        pub[0] = 4;
        p.Q.X!.CopyTo(pub, 1);
        p.Q.Y!.CopyTo(pub, 33);
        return (p.D!, pub);
    }

    internal static byte[] EcSign(byte[] priv, byte[] pub, byte[] d)
    {
        using var e = Ec(priv, pub);
        return e.SignData(d, System.Security.Cryptography.HashAlgorithmName.SHA256);
    }

    internal static bool EcVerify(byte[] pub, byte[] d, byte[] s)
    {
        if (pub.Length != 65 || s.Length != 64) return false;
        using var e = Ec(null, pub);
        return e.VerifyData(d, s, System.Security.Cryptography.HashAlgorithmName.SHA256);
    }

    internal static bool RsaVerify(byte[] n, byte[] e, byte[] d, byte[] s)
    {
        using var r = System.Security.Cryptography.RSA.Create();
        r.ImportParameters(new System.Security.Cryptography.RSAParameters { Modulus = n, Exponent = e });
        return r.VerifyData(d, s, System.Security.Cryptography.HashAlgorithmName.SHA256, System.Security.Cryptography.RSASignaturePadding.Pkcs1);
    }
}

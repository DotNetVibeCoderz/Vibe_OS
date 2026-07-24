// Buitenzorg.Bcl — a hand-written .NET-style base class library subset
// (v0.15 "Matang" increment 6). Built on the working managed heap (increment 4:
// `new`/arrays/generics work under zerolib), this gives ring-3 C# apps real,
// usable collections/text/encoding without the full CoreLib. It deliberately
// avoids the two zerolib limits: no static reference fields (GC statics) and no
// dynamically created strings (const string literals + char[] instead).

using System;
using System.Runtime.InteropServices;

namespace Buitenzorg
{
    /// <summary>Console output for dynamic text (writes a char[] as bytes).</summary>
    public static class Con
    {
        [DllImport("*")] static extern unsafe void bz_write(byte* buf, ulong len);

        public static unsafe void Write(char[] buf, int len)
        {
            byte* tmp = stackalloc byte[256];
            int i = 0;
            while (i < len)
            {
                int n = 0;
                while (n < 256 && i < len) tmp[n++] = (byte)buf[i++];
                bz_write(tmp, (ulong)n);
            }
        }
    }

    /// <summary>A growable generic list (like List&lt;T&gt;).</summary>
    public sealed class BzList<T>
    {
        T[] _a;
        int _n;
        public BzList() { _a = new T[4]; _n = 0; }
        public void Add(T v)
        {
            if (_n == _a.Length)
            {
                T[] b = new T[_a.Length * 2];
                for (int i = 0; i < _n; i++) b[i] = _a[i];
                _a = b;
            }
            _a[_n++] = v;
        }
        public int Count => _n;
        public T this[int i] { get => _a[i]; set => _a[i] = value; }
    }

    /// <summary>A growable LIFO stack (like Stack&lt;T&gt;).</summary>
    public sealed class BzStack<T>
    {
        T[] _a;
        int _n;
        public BzStack() { _a = new T[4]; _n = 0; }
        public void Push(T v)
        {
            if (_n == _a.Length)
            {
                T[] b = new T[_a.Length * 2];
                for (int i = 0; i < _n; i++) b[i] = _a[i];
                _a = b;
            }
            _a[_n++] = v;
        }
        public T Pop() => _a[--_n];
        public T Peek() => _a[_n - 1];
        public int Count => _n;
    }

    /// <summary>A growable FIFO queue (circular buffer, like Queue&lt;T&gt;).</summary>
    public sealed class BzQueue<T>
    {
        T[] _a;
        int _head, _tail, _n;
        public BzQueue() { _a = new T[4]; _head = 0; _tail = 0; _n = 0; }
        public void Enqueue(T v)
        {
            if (_n == _a.Length)
            {
                T[] b = new T[_a.Length * 2];
                for (int i = 0; i < _n; i++) b[i] = _a[(_head + i) % _a.Length];
                _a = b; _head = 0; _tail = _n;
            }
            _a[_tail] = v; _tail = (_tail + 1) % _a.Length; _n++;
        }
        public T Dequeue() { T v = _a[_head]; _head = (_head + 1) % _a.Length; _n--; return v; }
        public int Count => _n;
    }

    /// <summary>An int-keyed hash map (open addressing), like Dictionary&lt;int,V&gt;.
    /// Generic key hashing/equality is awkward under zerolib, so this specializes
    /// on int keys (the common case) with a generic value.</summary>
    public sealed class BzIntMap<V>
    {
        int[] _keys;
        V[] _vals;
        bool[] _used;
        int _n, _cap;
        public BzIntMap() { _cap = 8; _keys = new int[_cap]; _vals = new V[_cap]; _used = new bool[_cap]; _n = 0; }
        int Slot(int key)
        {
            int i = (key & 0x7fffffff) % _cap;
            while (_used[i] && _keys[i] != key) i = (i + 1) % _cap;
            return i;
        }
        void Grow()
        {
            int oc = _cap;
            int[] ok = _keys; V[] ov = _vals; bool[] ou = _used;
            _cap *= 2; _keys = new int[_cap]; _vals = new V[_cap]; _used = new bool[_cap];
            for (int i = 0; i < oc; i++)
            {
                if (ou[i]) { int s = Slot(ok[i]); _keys[s] = ok[i]; _vals[s] = ov[i]; _used[s] = true; }
            }
        }
        public void Set(int key, V val)
        {
            if ((_n + 1) * 2 >= _cap) Grow();
            int s = Slot(key);
            if (!_used[s]) _n++;
            _keys[s] = key; _vals[s] = val; _used[s] = true;
        }
        public bool TryGet(int key, out V val)
        {
            int s = Slot(key);
            if (_used[s]) { val = _vals[s]; return true; }
            val = default; return false;
        }
        public int Count => _n;
    }

    /// <summary>An int hash set (open addressing), like HashSet&lt;int&gt;.</summary>
    public sealed class BzIntSet
    {
        int[] _keys;
        bool[] _used;
        int _n, _cap;
        public BzIntSet() { _cap = 8; _keys = new int[_cap]; _used = new bool[_cap]; _n = 0; }
        int Slot(int key)
        {
            int i = (key & 0x7fffffff) % _cap;
            while (_used[i] && _keys[i] != key) i = (i + 1) % _cap;
            return i;
        }
        void Grow()
        {
            int oc = _cap; int[] ok = _keys; bool[] ou = _used;
            _cap *= 2; _keys = new int[_cap]; _used = new bool[_cap];
            for (int i = 0; i < oc; i++) if (ou[i]) { int s = Slot(ok[i]); _keys[s] = ok[i]; _used[s] = true; }
        }
        public bool Add(int key)
        {
            if ((_n + 1) * 2 >= _cap) Grow();
            int s = Slot(key);
            if (_used[s]) return false;
            _keys[s] = key; _used[s] = true; _n++;
            return true;
        }
        public bool Contains(int key) => _used[Slot(key)];
        public int Count => _n;
    }

    /// <summary>A string-keyed hash map (FNV-1a hash + char-wise equality),
    /// like Dictionary&lt;string,V&gt;.</summary>
    public sealed class BzStrMap<V>
    {
        string[] _keys;
        V[] _vals;
        bool[] _used;
        int _n, _cap;
        public BzStrMap() { _cap = 8; _keys = new string[_cap]; _vals = new V[_cap]; _used = new bool[_cap]; _n = 0; }
        static int Hash(string s)
        {
            unchecked
            {
                int h = (int)2166136261;
                for (int i = 0; i < s.Length; i++) { h = (h ^ s[i]) * 16777619; }
                return h & 0x7fffffff;
            }
        }
        static bool Eq(string a, string b)
        {
            if (a.Length != b.Length) return false;
            for (int i = 0; i < a.Length; i++) if (a[i] != b[i]) return false;
            return true;
        }
        int Slot(string key)
        {
            int i = Hash(key) % _cap;
            while (_used[i] && !Eq(_keys[i], key)) i = (i + 1) % _cap;
            return i;
        }
        void Grow()
        {
            int oc = _cap; string[] ok = _keys; V[] ov = _vals; bool[] ou = _used;
            _cap *= 2; _keys = new string[_cap]; _vals = new V[_cap]; _used = new bool[_cap];
            for (int i = 0; i < oc; i++) if (ou[i]) { int s = Slot(ok[i]); _keys[s] = ok[i]; _vals[s] = ov[i]; _used[s] = true; }
        }
        public void Set(string key, V val)
        {
            if ((_n + 1) * 2 >= _cap) Grow();
            int s = Slot(key);
            if (!_used[s]) _n++;
            _keys[s] = key; _vals[s] = val; _used[s] = true;
        }
        public bool TryGet(string key, out V val)
        {
            int s = Slot(key);
            if (_used[s]) { val = _vals[s]; return true; }
            val = default; return false;
        }
        public int Count => _n;
    }

    /// <summary>In-place quicksort over BzList&lt;int&gt;.</summary>
    public static class BzSort
    {
        public static void Sort(BzList<int> a) => QSort(a, 0, a.Count - 1);
        static void QSort(BzList<int> a, int lo, int hi)
        {
            if (lo >= hi) return;
            int pivot = a[(lo + hi) / 2];
            int i = lo, j = hi;
            while (i <= j)
            {
                while (a[i] < pivot) i++;
                while (a[j] > pivot) j--;
                if (i <= j) { int t = a[i]; a[i] = a[j]; a[j] = t; i++; j--; }
            }
            QSort(a, lo, j);
            QSort(a, i, hi);
        }
    }

    /// <summary>A growable char-buffer text builder (like StringBuilder).</summary>
    public sealed class BzStringBuilder
    {
        char[] _a;
        int _n;
        public BzStringBuilder() { _a = new char[16]; _n = 0; }
        void Ensure(int extra)
        {
            if (_n + extra > _a.Length)
            {
                int cap = _a.Length * 2;
                while (cap < _n + extra) cap *= 2;
                char[] b = new char[cap];
                for (int i = 0; i < _n; i++) b[i] = _a[i];
                _a = b;
            }
        }
        public BzStringBuilder Append(char c) { Ensure(1); _a[_n++] = c; return this; }
        public BzStringBuilder Append(string s) { Ensure(s.Length); for (int i = 0; i < s.Length; i++) _a[_n++] = s[i]; return this; }
        public BzStringBuilder Append(int v)
        {
            if (v == 0) return Append('0');
            bool neg = v < 0;
            long x = v; if (neg) x = -x;
            char[] tmp = new char[16];
            int t = 0;
            while (x > 0) { tmp[t++] = (char)('0' + (int)(x % 10)); x /= 10; }
            if (neg) Append('-');
            for (int i = t - 1; i >= 0; i--) Append(tmp[i]);
            return this;
        }
        public BzStringBuilder AppendLong(long v)
        {
            if (v == 0) return Append('0');
            bool neg = v < 0;
            ulong x = neg ? (ulong)(-v) : (ulong)v;
            char[] tmp = new char[24];
            int t = 0;
            while (x > 0) { tmp[t++] = (char)('0' + (int)(x % 10)); x /= 10; }
            if (neg) Append('-');
            for (int i = t - 1; i >= 0; i--) Append(tmp[i]);
            return this;
        }
        public BzStringBuilder AppendHex(int v)
        {
            const string H = "0123456789abcdef";
            Append('0'); Append('x');
            bool started = false;
            for (int shift = 28; shift >= 0; shift -= 4)
            {
                int nib = (v >> shift) & 0xF;
                if (nib != 0 || started || shift == 0) { Append(H[nib]); started = true; }
            }
            return this;
        }
        public int Length => _n;
        public void Print() => Con.Write(_a, _n);
        public void Clear() => _n = 0;
        public BzStringBuilder AppendLine() => Append('\n');
        public BzStringBuilder AppendLine(string s) { Append(s); return Append('\n'); }
        public BzStringBuilder AppendChars(char[] s, int len) { Ensure(len); for (int i = 0; i < len; i++) _a[_n++] = s[i]; return this; }
        /// <summary>Copy the current contents into `dst`; returns the length.</summary>
        public int CopyTo(char[] dst) { int m = _n < dst.Length ? _n : dst.Length; for (int i = 0; i < m; i++) dst[i] = _a[i]; return m; }
    }

    /// <summary>LINQ-style operators over BzList&lt;int&gt;. Predicates/selectors
    /// are passed as function pointers (`delegate*&lt;int,bool&gt;`) rather than
    /// delegates: a method-group→delegate conversion caches the delegate in a
    /// GC static field, which zerolib doesn't initialize. Function pointers are
    /// plain code addresses — no allocation, no GC static.</summary>
    public static class BzLinq
    {
        public static unsafe BzList<int> Where(BzList<int> src, delegate*<int, bool> pred)
        {
            BzList<int> r = new BzList<int>();
            for (int i = 0; i < src.Count; i++) if (pred(src[i])) r.Add(src[i]);
            return r;
        }
        public static unsafe BzList<int> Select(BzList<int> src, delegate*<int, int> map)
        {
            BzList<int> r = new BzList<int>();
            for (int i = 0; i < src.Count; i++) r.Add(map(src[i]));
            return r;
        }
        public static int Sum(BzList<int> src)
        {
            int s = 0;
            for (int i = 0; i < src.Count; i++) s += src[i];
            return s;
        }
        public static unsafe int Count(BzList<int> src, delegate*<int, bool> pred)
        {
            int c = 0;
            for (int i = 0; i < src.Count; i++) if (pred(src[i])) c++;
            return c;
        }
        public static unsafe bool Any(BzList<int> src, delegate*<int, bool> pred)
        {
            for (int i = 0; i < src.Count; i++) if (pred(src[i])) return true;
            return false;
        }
        public static unsafe bool All(BzList<int> src, delegate*<int, bool> pred)
        {
            for (int i = 0; i < src.Count; i++) if (!pred(src[i])) return false;
            return true;
        }
        public static int Max(BzList<int> src)
        {
            int m = src[0];
            for (int i = 1; i < src.Count; i++) if (src[i] > m) m = src[i];
            return m;
        }
        public static int Min(BzList<int> src)
        {
            int m = src[0];
            for (int i = 1; i < src.Count; i++) if (src[i] < m) m = src[i];
            return m;
        }
        public static int IndexOf(BzList<int> src, int val)
        {
            for (int i = 0; i < src.Count; i++) if (src[i] == val) return i;
            return -1;
        }
        public static bool Contains(BzList<int> src, int val) => IndexOf(src, val) >= 0;
        public static BzList<int> Reverse(BzList<int> src)
        {
            BzList<int> r = new BzList<int>();
            for (int i = src.Count - 1; i >= 0; i--) r.Add(src[i]);
            return r;
        }
        public static int First(BzList<int> src) => src[0];
        public static int Last(BzList<int> src) => src[src.Count - 1];
        public static BzList<int> Take(BzList<int> src, int n)
        {
            BzList<int> r = new BzList<int>();
            int m = n < src.Count ? n : src.Count;
            for (int i = 0; i < m; i++) r.Add(src[i]);
            return r;
        }
        public static BzList<int> Skip(BzList<int> src, int n)
        {
            BzList<int> r = new BzList<int>();
            for (int i = n; i < src.Count; i++) r.Add(src[i]);
            return r;
        }
        public static int Average(BzList<int> src) => src.Count == 0 ? 0 : Sum(src) / src.Count;
        /// <summary>Fold with a binary function pointer, starting from `seed`.</summary>
        public static unsafe int Aggregate(BzList<int> src, int seed, delegate*<int, int, int> fn)
        {
            int acc = seed;
            for (int i = 0; i < src.Count; i++) acc = fn(acc, src[i]);
            return acc;
        }
    }

    /// <summary>Little-endian byte conversions (like BitConverter).</summary>
    public static class BzBitConverter
    {
        public static byte[] GetBytes(int v)
        {
            byte[] b = new byte[4];
            b[0] = (byte)v; b[1] = (byte)(v >> 8); b[2] = (byte)(v >> 16); b[3] = (byte)(v >> 24);
            return b;
        }
        public static int ToInt32(byte[] b, int o) =>
            b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24);
    }

    /// <summary>Base64 encoder (like Convert.ToBase64String).</summary>
    public static class BzBase64
    {
        const string T = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        public static char[] Encode(byte[] data, out int outLen)
        {
            int n = data.Length;
            int groups = (n + 2) / 3;
            char[] o = new char[groups * 4];
            int oi = 0, i = 0;
            while (i + 3 <= n)
            {
                int v = (data[i] << 16) | (data[i + 1] << 8) | data[i + 2];
                o[oi++] = T[(v >> 18) & 63];
                o[oi++] = T[(v >> 12) & 63];
                o[oi++] = T[(v >> 6) & 63];
                o[oi++] = T[v & 63];
                i += 3;
            }
            int rem = n - i;
            if (rem == 1)
            {
                int v = data[i] << 16;
                o[oi++] = T[(v >> 18) & 63];
                o[oi++] = T[(v >> 12) & 63];
                o[oi++] = '=';
                o[oi++] = '=';
            }
            else if (rem == 2)
            {
                int v = (data[i] << 16) | (data[i + 1] << 8);
                o[oi++] = T[(v >> 18) & 63];
                o[oi++] = T[(v >> 12) & 63];
                o[oi++] = T[(v >> 6) & 63];
                o[oi++] = '=';
            }
            outLen = oi;
            return o;
        }
    }

    /// <summary>Integer math helpers (like System.Math), no floats.</summary>
    public static class BzMath
    {
        public static int Abs(int v) => v < 0 ? -v : v;
        public static long AbsL(long v) => v < 0 ? -v : v;
        public static int Min(int a, int b) => a < b ? a : b;
        public static int Max(int a, int b) => a > b ? a : b;
        public static long MinL(long a, long b) => a < b ? a : b;
        public static long MaxL(long a, long b) => a > b ? a : b;
        public static int Clamp(int v, int lo, int hi) => v < lo ? lo : (v > hi ? hi : v);
        public static int Sign(int v) => v < 0 ? -1 : (v > 0 ? 1 : 0);
        public static long Pow(int b, int e) { long r = 1; while (e-- > 0) r *= b; return r; }
        public static int ISqrt(long v) { if (v <= 0) return 0; long x = v, y = (x + 1) / 2; while (y < x) { x = y; y = (x + v / x) / 2; } return (int)x; }
        public static int Gcd(int a, int b) { a = Abs(a); b = Abs(b); while (b != 0) { int t = b; b = a % b; a = t; } return a; }
        public static long Lcm(int a, int b) { int g = Gcd(a, b); return g == 0 ? 0 : (long)Abs(a / g) * Abs(b); }
    }

    /// <summary>A fast PRNG (xorshift64*). State is per-instance (no GC statics).</summary>
    public sealed class BzRandom
    {
        ulong _s;
        public BzRandom(ulong seed) { _s = seed == 0 ? 0x9E3779B97F4A7C15UL : seed; }
        public ulong NextU64() { ulong x = _s; x ^= x << 13; x ^= x >> 7; x ^= x << 17; _s = x; return x; }
        public int Next() => (int)(NextU64() & 0x7FFFFFFF);
        public int NextRange(int lo, int hi) { if (hi <= lo) return lo; return lo + (int)(NextU64() % (ulong)(hi - lo)); }
        public bool NextBool() => (NextU64() & 1) != 0;
    }

    /// <summary>Number parsing / formatting (no `int.Parse`/`ToString` under zerolib).</summary>
    public static class BzConvert
    {
        public static long ParseLong(string s, out bool ok)
        {
            ok = false;
            if ((object)s == null || s.Length == 0) return 0;
            int i = 0; bool neg = false;
            if (s[0] == '-') { neg = true; i = 1; } else if (s[0] == '+') i = 1;
            long v = 0; bool any = false;
            for (; i < s.Length; i++) { char c = s[i]; if (c < '0' || c > '9') return 0; v = v * 10 + (c - '0'); any = true; }
            if (!any) return 0; ok = true; return neg ? -v : v;
        }
        public static int ParseInt(string s, out bool ok) => (int)ParseLong(s, out ok);
        public static int ParseHex(string s, out bool ok)
        {
            ok = false; if ((object)s == null || s.Length == 0) return 0;
            int v = 0; bool any = false;
            for (int i = 0; i < s.Length; i++)
            {
                char c = s[i]; int d;
                if (c >= '0' && c <= '9') d = c - '0';
                else if (c >= 'a' && c <= 'f') d = c - 'a' + 10;
                else if (c >= 'A' && c <= 'F') d = c - 'A' + 10;
                else return 0;
                v = (v << 4) | d; any = true;
            }
            if (!any) return 0; ok = true; return v;
        }
        /// <summary>Write `v` (base 10) into `buf`; returns the length.</summary>
        public static int LongToChars(long v, char[] buf)
        {
            int i = 0; bool neg = v < 0; if (neg) v = -v;
            char[] tmp = new char[24]; int j = 0;
            if (v == 0) tmp[j++] = '0'; else while (v > 0) { tmp[j++] = (char)('0' + (int)(v % 10)); v /= 10; }
            if (neg) buf[i++] = '-';
            while (j > 0) buf[i++] = tmp[--j];
            return i;
        }
    }

    /// <summary>String / char utilities that return int/bool/char[] (no new-string).</summary>
    public static class BzStr
    {
        public static bool Equals(string a, string b)
        {
            if ((object)a == null || (object)b == null) return (object)a == (object)b;
            if (a.Length != b.Length) return false;
            for (int i = 0; i < a.Length; i++) if (a[i] != b[i]) return false;
            return true;
        }
        public static int Compare(string a, string b)
        {
            int n = a.Length < b.Length ? a.Length : b.Length;
            for (int i = 0; i < n; i++) if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
            return a.Length == b.Length ? 0 : (a.Length < b.Length ? -1 : 1);
        }
        public static int IndexOf(string s, char c) { for (int i = 0; i < s.Length; i++) if (s[i] == c) return i; return -1; }
        public static bool Contains(string s, char c) => IndexOf(s, c) >= 0;
        public static bool StartsWith(string s, string p) { if (p.Length > s.Length) return false; for (int i = 0; i < p.Length; i++) if (s[i] != p[i]) return false; return true; }
        public static bool EndsWith(string s, string p) { if (p.Length > s.Length) return false; int o = s.Length - p.Length; for (int i = 0; i < p.Length; i++) if (s[o + i] != p[i]) return false; return true; }
        public static int Count(string s, char c) { int n = 0; for (int i = 0; i < s.Length; i++) if (s[i] == c) n++; return n; }
        public static char Upper(char c) => (c >= 'a' && c <= 'z') ? (char)(c - 32) : c;
        public static char Lower(char c) => (c >= 'A' && c <= 'Z') ? (char)(c + 32) : c;
        public static bool IsDigit(char c) => c >= '0' && c <= '9';
        public static bool IsAlpha(char c) => (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z');
        public static bool IsSpace(char c) => c == ' ' || c == '\t' || c == '\n' || c == '\r';
    }

    /// <summary>A growable list of REFERENCE types (linked-list backed, so it
    /// avoids the zerolib object-array `stelem.ref` fault that BzList&lt;T&gt; hits
    /// for reference elements).</summary>
    public sealed class BzRefList<T>
    {
        sealed class Node { public T V; public Node Next; }
        Node _head, _tail;
        int _n;
        public void Add(T v) { Node node = new Node(); node.V = v; if (_tail == null) { _head = node; _tail = node; } else { _tail.Next = node; _tail = node; } _n++; }
        public int Count => _n;
        public T Get(int i) { Node c = _head; while (i-- > 0 && c != null) c = c.Next; return c == null ? default(T) : c.V; }
    }

    /// <summary>Hex encoding (bytes -> lowercase hex chars).</summary>
    public static class BzHex
    {
        const string H = "0123456789abcdef";
        public static char[] Encode(byte[] data, out int outLen)
        {
            char[] o = new char[data.Length * 2]; int oi = 0;
            for (int i = 0; i < data.Length; i++) { o[oi++] = H[(data[i] >> 4) & 15]; o[oi++] = H[data[i] & 15]; }
            outLen = oi; return o;
        }
    }
}

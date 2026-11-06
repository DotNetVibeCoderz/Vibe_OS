// Buitenzorg.Bcl, part 3 (v0.15 "Matang"): the pieces of the .NET base library
// that became possible once the startup shim initializes GC statics (delegates
// and lambdas work) and implements type casts and covariant array stores.
//
//   System.Func / Action / Predicate / Comparison  delegate types zerolib lacks
//   BzInterlocked   Interlocked: Increment/Decrement/Add/Exchange/CompareExchange
//   BzTimeSpan      TimeSpan: 100 ns ticks, components, arithmetic, "c" format, parse
//   BzGuid          Guid: random v4 from the kernel DRBG (syscall RANDOM), "D" format, parse
//   BzEnumerable    LINQ over BzList<T> as extension methods: Where, Select,
//                   OrderBy/OrderByDescending/ThenBy (stable), GroupBy, Distinct,
//                   DistinctBy, Any/All/Count/First/Last, Sum/Min/Max by key,
//                   Aggregate, Take/Skip, Reverse, Concat, Zip, SequenceEqual, ToArray
//
// Uses BzList<T> and Con from bzbcl.cs. Text is char[] (zerolib has no string
// building). Comparisons go through keys (long) or Comparison<T>: zerolib's
// primitives implement no IComparable / IEquatable.

using System;
using System.Runtime.InteropServices;

namespace System
{
    public delegate TResult Func<TResult>();
    public delegate TResult Func<T, TResult>(T arg);
    public delegate TResult Func<T1, T2, TResult>(T1 arg1, T2 arg2);
    public delegate TResult Func<T1, T2, T3, TResult>(T1 arg1, T2 arg2, T3 arg3);
    public delegate void Action();
    public delegate void Action<T>(T arg);
    public delegate void Action<T1, T2>(T1 arg1, T2 arg2);
    public delegate void Action<T1, T2, T3>(T1 arg1, T2 arg2, T3 arg3);
    public delegate bool Predicate<T>(T obj);
    public delegate int Comparison<T>(T x, T y);
}

namespace Buitenzorg
{
    /// <summary>Atomic operations (like System.Threading.Interlocked), backed by the
    /// shim's bz_atomic_* exports (CPU atomics; threads run in parallel on SMP).</summary>
    public static unsafe class BzInterlocked
    {
        [DllImport("*")] static extern int bz_atomic_add32(int* p, int v);
        [DllImport("*")] static extern long bz_atomic_add(long* p, long v);
        [DllImport("*")] static extern int bz_atomic_cas32(int* p, int expected, int value);
        [DllImport("*")] static extern long bz_atomic_cas64(long* p, long expected, long value);
        [DllImport("*")] static extern int bz_atomic_xchg32(int* p, int value);
        [DllImport("*")] static extern long bz_atomic_xchg64(long* p, long value);

        /// <summary>Adds 1; returns the new value.</summary>
        public static int Increment(ref int location) { fixed (int* p = &location) return bz_atomic_add32(p, 1) + 1; }
        public static long Increment(ref long location) { fixed (long* p = &location) return bz_atomic_add(p, 1) + 1; }
        /// <summary>Subtracts 1; returns the new value.</summary>
        public static int Decrement(ref int location) { fixed (int* p = &location) return bz_atomic_add32(p, -1) - 1; }
        public static long Decrement(ref long location) { fixed (long* p = &location) return bz_atomic_add(p, -1) - 1; }
        /// <summary>Adds <paramref name="value"/>; returns the new value.</summary>
        public static int Add(ref int location, int value) { fixed (int* p = &location) return bz_atomic_add32(p, value) + value; }
        public static long Add(ref long location, long value) { fixed (long* p = &location) return bz_atomic_add(p, value) + value; }
        /// <summary>Stores <paramref name="value"/>; returns the previous value.</summary>
        public static int Exchange(ref int location, int value) { fixed (int* p = &location) return bz_atomic_xchg32(p, value); }
        public static long Exchange(ref long location, long value) { fixed (long* p = &location) return bz_atomic_xchg64(p, value); }
        /// <summary>Stores <paramref name="value"/> if the location holds
        /// <paramref name="comparand"/>; returns the value that was there (the .NET argument order).</summary>
        public static int CompareExchange(ref int location, int value, int comparand) { fixed (int* p = &location) return bz_atomic_cas32(p, comparand, value); }
        public static long CompareExchange(ref long location, long value, long comparand) { fixed (long* p = &location) return bz_atomic_cas64(p, comparand, value); }
        /// <summary>An atomic 64-bit read.</summary>
        public static long Read(ref long location) { fixed (long* p = &location) return bz_atomic_add(p, 0); }
    }

    /// <summary>A time interval in 100 ns ticks (like System.TimeSpan).</summary>
    public struct BzTimeSpan
    {
        public const long TicksPerMillisecond = 10_000;
        public const long TicksPerSecond = 10_000_000;
        public const long TicksPerMinute = 600_000_000;
        public const long TicksPerHour = 36_000_000_000;
        public const long TicksPerDay = 864_000_000_000;

        public readonly long Ticks;

        public BzTimeSpan(long ticks) { Ticks = ticks; }
        public BzTimeSpan(int hours, int minutes, int seconds)
        {
            Ticks = hours * TicksPerHour + minutes * TicksPerMinute + seconds * TicksPerSecond;
        }
        public BzTimeSpan(int days, int hours, int minutes, int seconds, int milliseconds)
        {
            Ticks = days * TicksPerDay + hours * TicksPerHour + minutes * TicksPerMinute
                  + seconds * TicksPerSecond + milliseconds * TicksPerMillisecond;
        }

        public static BzTimeSpan Zero => new BzTimeSpan(0);
        public static BzTimeSpan FromTicks(long t) => new BzTimeSpan(t);
        public static BzTimeSpan FromMilliseconds(long ms) => new BzTimeSpan(ms * TicksPerMillisecond);
        public static BzTimeSpan FromSeconds(long s) => new BzTimeSpan(s * TicksPerSecond);
        public static BzTimeSpan FromMinutes(long m) => new BzTimeSpan(m * TicksPerMinute);
        public static BzTimeSpan FromHours(long h) => new BzTimeSpan(h * TicksPerHour);
        public static BzTimeSpan FromDays(long d) => new BzTimeSpan(d * TicksPerDay);
        /// <summary>From a nanosecond count (e.g. CLOCK_NANOS differences).</summary>
        public static BzTimeSpan FromNanoseconds(long ns) => new BzTimeSpan(ns / 100);

        public int Days => (int)(Ticks / TicksPerDay);
        public int Hours => (int)(Ticks / TicksPerHour % 24);
        public int Minutes => (int)(Ticks / TicksPerMinute % 60);
        public int Seconds => (int)(Ticks / TicksPerSecond % 60);
        public int Milliseconds => (int)(Ticks / TicksPerMillisecond % 1000);
        public long TotalMilliseconds => Ticks / TicksPerMillisecond;
        public long TotalSeconds => Ticks / TicksPerSecond;
        public long TotalMinutes => Ticks / TicksPerMinute;
        public long TotalHours => Ticks / TicksPerHour;
        public long TotalDays => Ticks / TicksPerDay;

        public BzTimeSpan Add(BzTimeSpan o) => new BzTimeSpan(Ticks + o.Ticks);
        public BzTimeSpan Subtract(BzTimeSpan o) => new BzTimeSpan(Ticks - o.Ticks);
        public BzTimeSpan Negate() => new BzTimeSpan(-Ticks);
        public BzTimeSpan Duration() => new BzTimeSpan(Ticks < 0 ? -Ticks : Ticks);
        public int CompareTo(BzTimeSpan o) => Ticks < o.Ticks ? -1 : Ticks > o.Ticks ? 1 : 0;
        public bool Equals(BzTimeSpan o) => Ticks == o.Ticks;

        public static BzTimeSpan operator +(BzTimeSpan a, BzTimeSpan b) => new BzTimeSpan(a.Ticks + b.Ticks);
        public static BzTimeSpan operator -(BzTimeSpan a, BzTimeSpan b) => new BzTimeSpan(a.Ticks - b.Ticks);
        public static BzTimeSpan operator -(BzTimeSpan a) => new BzTimeSpan(-a.Ticks);
        public static bool operator ==(BzTimeSpan a, BzTimeSpan b) => a.Ticks == b.Ticks;
        public static bool operator !=(BzTimeSpan a, BzTimeSpan b) => a.Ticks != b.Ticks;
        public static bool operator <(BzTimeSpan a, BzTimeSpan b) => a.Ticks < b.Ticks;
        public static bool operator >(BzTimeSpan a, BzTimeSpan b) => a.Ticks > b.Ticks;
        public static bool operator <=(BzTimeSpan a, BzTimeSpan b) => a.Ticks <= b.Ticks;
        public static bool operator >=(BzTimeSpan a, BzTimeSpan b) => a.Ticks >= b.Ticks;

        static int Put2(char[] b, int p, int v) { b[p] = (char)('0' + v / 10); b[p + 1] = (char)('0' + v % 10); return p + 2; }

        /// <summary>Writes the .NET "c" format, <c>[-][d.]hh:mm:ss[.fffffff]</c>, at
        /// <paramref name="pos"/>; returns the position after it (needs up to 26 chars).</summary>
        public int Format(char[] buf, int pos)
        {
            long t = Ticks;
            if (t < 0) { buf[pos++] = '-'; t = -t; }
            long days = t / TicksPerDay;
            if (days > 0)
            {
                long div = 1;
                while (div * 10 <= days) div *= 10;
                for (; div > 0; div /= 10) buf[pos++] = (char)('0' + days / div % 10);
                buf[pos++] = '.';
            }
            pos = Put2(buf, pos, (int)(t / TicksPerHour % 24)); buf[pos++] = ':';
            pos = Put2(buf, pos, (int)(t / TicksPerMinute % 60)); buf[pos++] = ':';
            pos = Put2(buf, pos, (int)(t / TicksPerSecond % 60));
            long frac = t % TicksPerSecond;
            if (frac != 0)
            {
                buf[pos++] = '.';
                long div = 1_000_000;
                for (int i = 0; i < 7; i++) { buf[pos++] = (char)('0' + frac / div % 10); div /= 10; }
            }
            return pos;
        }

        /// <summary>Parses <c>[-][d.]hh:mm[:ss[.fffffff]]</c> from <paramref name="s"/>[0..len).</summary>
        public static bool TryParse(char[] s, int len, out BzTimeSpan result)
        {
            result = Zero;
            int i = 0;
            bool neg = false;
            if (i < len && s[i] == '-') { neg = true; i++; }
            long[] parts = new long[4]; // d, h, m, s
            int n = 0;
            bool sawDays = false;
            long cur = 0;
            int digits = 0;
            long frac = 0;
            int fracDigits = 0;
            bool inFrac = false;
            for (; i < len; i++)
            {
                char c = s[i];
                if (c >= '0' && c <= '9')
                {
                    if (inFrac) { if (fracDigits < 7) { frac = frac * 10 + (c - '0'); fracDigits++; } }
                    else { cur = cur * 10 + (c - '0'); digits++; }
                }
                else if (c == '.' && !inFrac && n == 0 && !sawDays && digits > 0) { parts[0] = cur; sawDays = true; cur = 0; digits = 0; }
                else if (c == ':' && !inFrac && digits > 0 && n < 2) { parts[1 + n] = cur; n++; cur = 0; digits = 0; }
                else if (c == '.' && !inFrac && n == 2 && digits > 0) { parts[3] = cur; n++; cur = 0; digits = 0; inFrac = true; }
                else return false;
            }
            if (inFrac) { /* seconds already stored */ }
            else if (digits > 0 && n >= 1 && n <= 2) { parts[1 + n] = cur; n++; }
            else return false;
            if (n < 2) return false;
            if (parts[1] > 23 || parts[2] > 59 || parts[3] > 59) return false;
            while (fracDigits < 7) { frac *= 10; fracDigits++; }
            long t = parts[0] * TicksPerDay + parts[1] * TicksPerHour + parts[2] * TicksPerMinute + parts[3] * TicksPerSecond + frac;
            result = new BzTimeSpan(neg ? -t : t);
            return true;
        }
    }

    /// <summary>A 128-bit globally unique identifier (like System.Guid; same byte order).</summary>
    public struct BzGuid
    {
        readonly ulong _lo; // bytes 0..7
        readonly ulong _hi; // bytes 8..15

        [DllImport("*")] static extern unsafe ulong bz_random(byte* buf, ulong len);

        BzGuid(ulong lo, ulong hi) { _lo = lo; _hi = hi; }

        public static BzGuid Empty => new BzGuid(0, 0);
        public bool IsEmpty => _lo == 0 && _hi == 0;

        /// <summary>A random (version 4) GUID from the kernel's ChaCha20 DRBG.</summary>
        public static unsafe BzGuid NewGuid()
        {
            byte* b = stackalloc byte[16];
            if (bz_random(b, 16) != 16) return Empty;
            b[7] = (byte)((b[7] & 0x0F) | 0x40); // version 4 (high nibble of field c)
            b[8] = (byte)((b[8] & 0x3F) | 0x80); // RFC 4122 variant
            return FromBytes(b);
        }

        static unsafe BzGuid FromBytes(byte* b)
        {
            ulong lo = 0, hi = 0;
            for (int i = 0; i < 8; i++) { lo |= (ulong)b[i] << (8 * i); hi |= (ulong)b[8 + i] << (8 * i); }
            return new BzGuid(lo, hi);
        }

        public static unsafe BzGuid FromByteArray(byte[] b)
        {
            if (b == null || b.Length < 16) return Empty;
            fixed (byte* p = b) return FromBytes(p);
        }

        public byte this[int i] => (byte)((i < 8 ? _lo >> (8 * i) : _hi >> (8 * (i - 8))) & 0xFF);

        public void ToByteArray(byte[] dst)
        {
            for (int i = 0; i < 16; i++) dst[i] = this[i];
        }

        public int Version => this[7] >> 4;

        public bool Equals(BzGuid o) => _lo == o._lo && _hi == o._hi;
        public static bool operator ==(BzGuid a, BzGuid b) => a._lo == b._lo && a._hi == b._hi;
        public static bool operator !=(BzGuid a, BzGuid b) => !(a == b);

        static int Hex(char[] buf, int pos, int v) { buf[pos] = (char)(v < 10 ? '0' + v : 'a' + v - 10); return pos + 1; }
        int PutByte(char[] buf, int pos, int index) { int v = this[index]; pos = Hex(buf, pos, v >> 4); return Hex(buf, pos, v & 15); }

        /// <summary>Writes the "D" format (36 chars, lower case, e.g.
        /// <c>0f8fad5b-d9cb-469f-a165-70867728950e</c>); returns the position after it.</summary>
        public int Format(char[] buf, int pos)
        {
            // the first three fields are little-endian integers in the byte layout
            for (int i = 3; i >= 0; i--) pos = PutByte(buf, pos, i);
            buf[pos++] = '-';
            pos = PutByte(buf, pos, 5); pos = PutByte(buf, pos, 4);
            buf[pos++] = '-';
            pos = PutByte(buf, pos, 7); pos = PutByte(buf, pos, 6);
            buf[pos++] = '-';
            pos = PutByte(buf, pos, 8); pos = PutByte(buf, pos, 9);
            buf[pos++] = '-';
            for (int i = 10; i < 16; i++) pos = PutByte(buf, pos, i);
            return pos;
        }

        static int HexVal(char c)
        {
            if (c >= '0' && c <= '9') return c - '0';
            if (c >= 'a' && c <= 'f') return c - 'a' + 10;
            if (c >= 'A' && c <= 'F') return c - 'A' + 10;
            return -1;
        }

        /// <summary>Parses the "D" format (36 chars, optionally in braces) at <paramref name="start"/>.</summary>
        public static unsafe bool TryParse(char[] s, int start, int len, out BzGuid g)
        {
            g = Empty;
            if (len == 38 && s[start] == '{' && s[start + 37] == '}') { start++; len = 36; }
            if (len != 36) return false;
            if (s[start + 8] != '-' || s[start + 13] != '-' || s[start + 18] != '-' || s[start + 23] != '-') return false;
            byte* b = stackalloc byte[16];
            // text position of each byte's high nibble, in byte order
            int* at = stackalloc int[16];
            at[3] = 0; at[2] = 2; at[1] = 4; at[0] = 6;
            at[5] = 9; at[4] = 11;
            at[7] = 14; at[6] = 16;
            at[8] = 19; at[9] = 21;
            for (int i = 0; i < 6; i++) at[10 + i] = 24 + 2 * i;
            for (int i = 0; i < 16; i++)
            {
                int hi = HexVal(s[start + at[i]]), lo = HexVal(s[start + at[i] + 1]);
                if (hi < 0 || lo < 0) return false;
                b[i] = (byte)(hi << 4 | lo);
            }
            g = FromBytes(b);
            return true;
        }
    }

    /// <summary>One group of <see cref="BzEnumerable.GroupBy"/>: a key and its items in source order.</summary>
    public sealed class BzGrouping<T>
    {
        public readonly long Key;
        public readonly BzList<T> Items = new BzList<T>();
        public BzGrouping(long key) { Key = key; }
        public int Count => Items.Count;
    }

    /// <summary>A set of 64-bit keys (open addressing), used by Distinct/GroupBy.</summary>
    public sealed class BzLongSet
    {
        long[] _keys = new long[16];
        int[] _vals = new int[16];
        bool[] _used = new bool[16];
        int _n;

        public int Count => _n;

        int Slot(long k, long[] keys, bool[] used)
        {
            ulong h = (ulong)k * 0x9E3779B97F4A7C15UL;
            int mask = keys.Length - 1;
            int i = (int)(h >> 40) & mask;
            while (used[i] && keys[i] != k) i = (i + 1) & mask;
            return i;
        }

        void Grow()
        {
            long[] ok = _keys; int[] ov = _vals; bool[] ou = _used;
            _keys = new long[ok.Length * 2]; _vals = new int[ok.Length * 2]; _used = new bool[ok.Length * 2];
            for (int i = 0; i < ok.Length; i++)
                if (ou[i]) { int s = Slot(ok[i], _keys, _used); _keys[s] = ok[i]; _vals[s] = ov[i]; _used[s] = true; }
        }

        /// <summary>Adds <paramref name="k"/> with <paramref name="value"/>; false if it was there.</summary>
        public bool Add(long k, int value)
        {
            if ((_n + 1) * 4 > _keys.Length * 3) Grow();
            int s = Slot(k, _keys, _used);
            if (_used[s]) return false;
            _keys[s] = k; _vals[s] = value; _used[s] = true; _n++;
            return true;
        }
        public bool Add(long k) => Add(k, 0);
        public bool Contains(long k) => _used[Slot(k, _keys, _used)];
        /// <summary>The value stored with <paramref name="k"/>, or -1.</summary>
        public int Get(long k) { int s = Slot(k, _keys, _used); return _used[s] ? _vals[s] : -1; }
    }

    /// <summary>LINQ-style operators over <see cref="BzList{T}"/> (extension methods,
    /// so they chain: <c>list.Where(x =&gt; x &gt; 2).OrderBy(x =&gt; x).Take(3)</c>).
    /// Every operator is eager and returns a new list.</summary>
    public static class BzEnumerable
    {
        public static BzList<T> Where<T>(this BzList<T> src, Predicate<T> pred)
        {
            var r = new BzList<T>();
            for (int i = 0; i < src.Count; i++) if (pred(src[i])) r.Add(src[i]);
            return r;
        }

        public static BzList<R> Select<T, R>(this BzList<T> src, Func<T, R> map)
        {
            var r = new BzList<R>();
            for (int i = 0; i < src.Count; i++) r.Add(map(src[i]));
            return r;
        }

        public static BzList<R> SelectIndexed<T, R>(this BzList<T> src, Func<T, int, R> map)
        {
            var r = new BzList<R>();
            for (int i = 0; i < src.Count; i++) r.Add(map(src[i], i));
            return r;
        }

        public static void ForEach<T>(this BzList<T> src, Action<T> action)
        {
            for (int i = 0; i < src.Count; i++) action(src[i]);
        }

        public static bool Any<T>(this BzList<T> src, Predicate<T> pred)
        {
            for (int i = 0; i < src.Count; i++) if (pred(src[i])) return true;
            return false;
        }

        public static bool All<T>(this BzList<T> src, Predicate<T> pred)
        {
            for (int i = 0; i < src.Count; i++) if (!pred(src[i])) return false;
            return true;
        }

        public static int Count<T>(this BzList<T> src, Predicate<T> pred)
        {
            int n = 0;
            for (int i = 0; i < src.Count; i++) if (pred(src[i])) n++;
            return n;
        }

        /// <summary>The first match, or <paramref name="fallback"/> (FirstOrDefault).</summary>
        public static T FirstOr<T>(this BzList<T> src, Predicate<T> pred, T fallback)
        {
            for (int i = 0; i < src.Count; i++) if (pred(src[i])) return src[i];
            return fallback;
        }

        public static T LastOr<T>(this BzList<T> src, Predicate<T> pred, T fallback)
        {
            for (int i = src.Count - 1; i >= 0; i--) if (pred(src[i])) return src[i];
            return fallback;
        }

        public static int IndexWhere<T>(this BzList<T> src, Predicate<T> pred)
        {
            for (int i = 0; i < src.Count; i++) if (pred(src[i])) return i;
            return -1;
        }

        public static long Sum<T>(this BzList<T> src, Func<T, long> value)
        {
            long s = 0;
            for (int i = 0; i < src.Count; i++) s += value(src[i]);
            return s;
        }

        /// <summary>The element with the smallest key (MinBy); <paramref name="fallback"/> when empty.</summary>
        public static T MinBy<T>(this BzList<T> src, Func<T, long> key, T fallback)
        {
            if (src.Count == 0) return fallback;
            T best = src[0]; long bk = key(best);
            for (int i = 1; i < src.Count; i++) { long k = key(src[i]); if (k < bk) { bk = k; best = src[i]; } }
            return best;
        }

        public static T MaxBy<T>(this BzList<T> src, Func<T, long> key, T fallback)
        {
            if (src.Count == 0) return fallback;
            T best = src[0]; long bk = key(best);
            for (int i = 1; i < src.Count; i++) { long k = key(src[i]); if (k > bk) { bk = k; best = src[i]; } }
            return best;
        }

        public static A Aggregate<T, A>(this BzList<T> src, A seed, Func<A, T, A> fn)
        {
            A acc = seed;
            for (int i = 0; i < src.Count; i++) acc = fn(acc, src[i]);
            return acc;
        }

        public static BzList<T> Take<T>(this BzList<T> src, int n)
        {
            var r = new BzList<T>();
            for (int i = 0; i < src.Count && i < n; i++) r.Add(src[i]);
            return r;
        }

        public static BzList<T> Skip<T>(this BzList<T> src, int n)
        {
            var r = new BzList<T>();
            for (int i = n < 0 ? 0 : n; i < src.Count; i++) r.Add(src[i]);
            return r;
        }

        public static BzList<T> TakeWhile<T>(this BzList<T> src, Predicate<T> pred)
        {
            var r = new BzList<T>();
            for (int i = 0; i < src.Count && pred(src[i]); i++) r.Add(src[i]);
            return r;
        }

        public static BzList<T> Reverse<T>(this BzList<T> src)
        {
            var r = new BzList<T>();
            for (int i = src.Count - 1; i >= 0; i--) r.Add(src[i]);
            return r;
        }

        public static BzList<T> Concat<T>(this BzList<T> a, BzList<T> b)
        {
            var r = new BzList<T>();
            for (int i = 0; i < a.Count; i++) r.Add(a[i]);
            for (int i = 0; i < b.Count; i++) r.Add(b[i]);
            return r;
        }

        public static BzList<R> Zip<A, B, R>(this BzList<A> a, BzList<B> b, Func<A, B, R> fn)
        {
            var r = new BzList<R>();
            for (int i = 0; i < a.Count && i < b.Count; i++) r.Add(fn(a[i], b[i]));
            return r;
        }

        public static bool SequenceEqual<T>(this BzList<T> a, BzList<T> b, Func<T, T, bool> eq)
        {
            if (a.Count != b.Count) return false;
            for (int i = 0; i < a.Count; i++) if (!eq(a[i], b[i])) return false;
            return true;
        }

        public static T[] ToArray<T>(this BzList<T> src)
        {
            var r = new T[src.Count];
            for (int i = 0; i < src.Count; i++) r[i] = src[i];
            return r;
        }

        public static BzList<T> ToBzList<T>(this T[] src)
        {
            var r = new BzList<T>();
            for (int i = 0; i < src.Length; i++) r.Add(src[i]);
            return r;
        }

        // ---- ordering: stable merge sort --------------------------------------

        static void MergeSort<T>(T[] a, T[] tmp, int lo, int hi, Comparison<T> cmp)
        {
            if (hi - lo < 2) return;
            int mid = (lo + hi) / 2;
            MergeSort(a, tmp, lo, mid, cmp);
            MergeSort(a, tmp, mid, hi, cmp);
            int i = lo, j = mid, k = lo;
            while (i < mid && j < hi) tmp[k++] = cmp(a[j], a[i]) < 0 ? a[j++] : a[i++];
            while (i < mid) tmp[k++] = a[i++];
            while (j < hi) tmp[k++] = a[j++];
            for (k = lo; k < hi; k++) a[k] = tmp[k];
        }

        /// <summary>Sorted copy by a comparison; stable (equal elements keep their order).</summary>
        public static BzList<T> OrderBy<T>(this BzList<T> src, Comparison<T> cmp)
        {
            T[] a = src.ToArray();
            MergeSort(a, new T[a.Length], 0, a.Length, cmp);
            return a.ToBzList();
        }

        /// <summary>Sorted copy by a numeric key; stable.</summary>
        public static BzList<T> OrderBy<T>(this BzList<T> src, Func<T, long> key)
            => src.OrderBy<T>((x, y) => Compare(key(x), key(y)));

        public static BzList<T> OrderByDescending<T>(this BzList<T> src, Func<T, long> key)
            => src.OrderBy<T>((x, y) => Compare(key(y), key(x)));

        public static BzList<T> OrderByDescending<T>(this BzList<T> src, Comparison<T> cmp)
            => src.OrderBy<T>((x, y) => cmp(y, x));

        /// <summary>A comparison that breaks ties of <paramref name="first"/> with
        /// <paramref name="then"/> (OrderBy(..).ThenBy(..) in one sort).</summary>
        public static Comparison<T> ThenBy<T>(this Comparison<T> first, Func<T, long> then)
            => (x, y) => { int c = first(x, y); return c != 0 ? c : Compare(then(x), then(y)); };

        public static Comparison<T> ThenByDescending<T>(this Comparison<T> first, Func<T, long> then)
            => (x, y) => { int c = first(x, y); return c != 0 ? c : Compare(then(y), then(x)); };

        /// <summary>A comparison by a numeric key, the start of a ThenBy chain.</summary>
        public static Comparison<T> By<T>(Func<T, long> key) => (x, y) => Compare(key(x), key(y));

        public static int Compare(long a, long b) => a < b ? -1 : a > b ? 1 : 0;

        /// <summary>Ordinal comparison of two strings (for string keys in a Comparison).</summary>
        public static int CompareOrdinal(string a, string b)
        {
            if ((object)a == (object)b) return 0;
            if (a == null) return -1;
            if (b == null) return 1;
            int n = a.Length < b.Length ? a.Length : b.Length;
            for (int i = 0; i < n; i++) if (a[i] != b[i]) return a[i] < b[i] ? -1 : 1;
            return Compare(a.Length, b.Length);
        }

        // ---- sets and groups --------------------------------------------------

        /// <summary>First occurrence of each key, in source order.</summary>
        public static BzList<T> DistinctBy<T>(this BzList<T> src, Func<T, long> key)
        {
            var seen = new BzLongSet();
            var r = new BzList<T>();
            for (int i = 0; i < src.Count; i++) if (seen.Add(key(src[i]))) r.Add(src[i]);
            return r;
        }

        public static BzList<int> Distinct(this BzList<int> src)
        {
            var seen = new BzLongSet();
            var r = new BzList<int>();
            for (int i = 0; i < src.Count; i++) if (seen.Add(src[i])) r.Add(src[i]);
            return r;
        }

        public static BzList<long> Distinct(this BzList<long> src)
        {
            var seen = new BzLongSet();
            var r = new BzList<long>();
            for (int i = 0; i < src.Count; i++) if (seen.Add(src[i])) r.Add(src[i]);
            return r;
        }

        /// <summary>Distinct with an equality function (O(n²); use DistinctBy for big lists).</summary>
        public static BzList<T> Distinct<T>(this BzList<T> src, Func<T, T, bool> eq)
        {
            var r = new BzList<T>();
            for (int i = 0; i < src.Count; i++)
            {
                bool dup = false;
                for (int j = 0; j < r.Count && !dup; j++) dup = eq(r[j], src[i]);
                if (!dup) r.Add(src[i]);
            }
            return r;
        }

        /// <summary>Groups by a numeric key; groups in order of first appearance,
        /// items in source order.</summary>
        public static BzList<BzGrouping<T>> GroupBy<T>(this BzList<T> src, Func<T, long> key)
        {
            var index = new BzLongSet();
            var groups = new BzList<BzGrouping<T>>();
            for (int i = 0; i < src.Count; i++)
            {
                long k = key(src[i]);
                int g = index.Get(k);
                if (g < 0)
                {
                    g = groups.Count;
                    index.Add(k, g);
                    groups.Add(new BzGrouping<T>(k));
                }
                groups[g].Items.Add(src[i]);
            }
            return groups;
        }
    }
}

// Buitenzorg.Bcl part 2 — the .NET namespaces the pre-v1.0 plan calls for:
// System.IO, System.Text, System.Text.RegularExpressions, System.Globalization,
// System.Diagnostics, System.Management, System.Threading.Tasks, System.Timers,
// GC and a Pkg API. Written by hand on the working managed heap, on top of the
// syscalls the kernel actually has.
//
// Same zerolib rules as bzbcl.cs and everything else in ring 3:
//   * no static reference fields (GC statics are uninitialized)  -> instance state
//   * no method-group -> delegate                                -> function pointers
//   * no storing a reference into an object[] element            -> linked lists
//   * no new string()/ToString()/concat/string ==                -> char[] buffers
//
// Compiled together with bzbcl.cs (this file uses Con/BzList/BzStringBuilder/...).

using System;
using System.Runtime.InteropServices;

namespace Buitenzorg
{
    // =====================================================================
    // System.Text — encoding
    // =====================================================================

    /// <summary>UTF-8 and ASCII transcoding between char[] and byte[]
    /// (System.Text.Encoding). Surrogate pairs are encoded as 4-byte UTF-8.</summary>
    public static class BzEncoding
    {
        /// <summary>Bytes needed to UTF-8 encode `src[0..len]`.</summary>
        public static int Utf8ByteCount(char[] src, int len)
        {
            int n = 0;
            for (int i = 0; i < len; i++)
            {
                int c = src[i];
                if (c < 0x80) n += 1;
                else if (c < 0x800) n += 2;
                else if (c >= 0xD800 && c <= 0xDBFF && i + 1 < len && src[i + 1] >= 0xDC00 && src[i + 1] <= 0xDFFF)
                { n += 4; i++; }
                else n += 3;
            }
            return n;
        }

        /// <summary>UTF-8 encode into `dst`; returns the byte count (-1 if `dst` is too small).</summary>
        public static int Utf8GetBytes(char[] src, int len, byte[] dst)
        {
            int n = 0;
            for (int i = 0; i < len; i++)
            {
                int c = src[i];
                if (c >= 0xD800 && c <= 0xDBFF && i + 1 < len && src[i + 1] >= 0xDC00 && src[i + 1] <= 0xDFFF)
                {
                    c = 0x10000 + ((c - 0xD800) << 10) + (src[i + 1] - 0xDC00);
                    i++;
                }
                if (c < 0x80)
                {
                    if (n + 1 > dst.Length) return -1;
                    dst[n++] = (byte)c;
                }
                else if (c < 0x800)
                {
                    if (n + 2 > dst.Length) return -1;
                    dst[n++] = (byte)(0xC0 | (c >> 6));
                    dst[n++] = (byte)(0x80 | (c & 0x3F));
                }
                else if (c < 0x10000)
                {
                    if (n + 3 > dst.Length) return -1;
                    dst[n++] = (byte)(0xE0 | (c >> 12));
                    dst[n++] = (byte)(0x80 | ((c >> 6) & 0x3F));
                    dst[n++] = (byte)(0x80 | (c & 0x3F));
                }
                else
                {
                    if (n + 4 > dst.Length) return -1;
                    dst[n++] = (byte)(0xF0 | (c >> 18));
                    dst[n++] = (byte)(0x80 | ((c >> 12) & 0x3F));
                    dst[n++] = (byte)(0x80 | ((c >> 6) & 0x3F));
                    dst[n++] = (byte)(0x80 | (c & 0x3F));
                }
            }
            return n;
        }

        /// <summary>UTF-8 encode a literal string. Returns the byte count.</summary>
        public static int Utf8GetBytes(string s, byte[] dst)
        {
            char[] tmp = new char[s.Length];
            for (int i = 0; i < s.Length; i++) tmp[i] = s[i];
            return Utf8GetBytes(tmp, s.Length, dst);
        }

        /// <summary>UTF-8 decode `src[0..len]` into `dst`; returns the char count
        /// (-1 if `dst` is too small). Malformed bytes decode to U+FFFD.</summary>
        public static int Utf8GetChars(byte[] src, int len, char[] dst)
        {
            int n = 0;
            int i = 0;
            while (i < len)
            {
                int b = src[i++];
                int cp;
                int extra;
                if (b < 0x80) { cp = b; extra = 0; }
                else if ((b & 0xE0) == 0xC0) { cp = b & 0x1F; extra = 1; }
                else if ((b & 0xF0) == 0xE0) { cp = b & 0x0F; extra = 2; }
                else if ((b & 0xF8) == 0xF0) { cp = b & 0x07; extra = 3; }
                else { cp = 0xFFFD; extra = 0; }

                bool bad = false;
                for (int k = 0; k < extra; k++)
                {
                    if (i >= len || (src[i] & 0xC0) != 0x80) { bad = true; break; }
                    cp = (cp << 6) | (src[i++] & 0x3F);
                }
                if (bad) cp = 0xFFFD;

                if (cp >= 0x10000)
                {
                    if (n + 2 > dst.Length) return -1;
                    cp -= 0x10000;
                    dst[n++] = (char)(0xD800 + (cp >> 10));
                    dst[n++] = (char)(0xDC00 + (cp & 0x3FF));
                }
                else
                {
                    if (n + 1 > dst.Length) return -1;
                    dst[n++] = (char)cp;
                }
            }
            return n;
        }

        /// <summary>ASCII encode (non-ASCII becomes '?'). Returns the byte count.</summary>
        public static int AsciiGetBytes(char[] src, int len, byte[] dst)
        {
            int m = len < dst.Length ? len : dst.Length;
            for (int i = 0; i < m; i++) dst[i] = src[i] < 0x80 ? (byte)src[i] : (byte)'?';
            return m;
        }

        /// <summary>ASCII decode. Returns the char count.</summary>
        public static int AsciiGetChars(byte[] src, int len, char[] dst)
        {
            int m = len < dst.Length ? len : dst.Length;
            for (int i = 0; i < m; i++) dst[i] = (char)(src[i] & 0x7F);
            return m;
        }
    }

    // =====================================================================
    // System.Text.RegularExpressions
    // =====================================================================

    /// <summary>One node of a compiled pattern. References live in fields
    /// (never in an object[] element), so the covariance check that faults
    /// under zerolib is never reached.</summary>
    public sealed class BzRxNode
    {
        public const int CHAR = 0;   // literal Ch
        public const int ANY = 1;    // .
        public const int CLASS = 2;  // [...] — Set/SetLen, Negate
        public const int GROUP = 3;  // (...) — Sub
        public const int BOL = 4;    // ^
        public const int EOL = 5;    // $

        public int Kind;
        public char Ch;
        public char[] Set;
        public int SetLen;
        public bool Negate;
        public BzRxNode Sub;      // GROUP body (an alternation)
        public BzRxNode Next;     // next node in this sequence
        public BzRxNode Alt;      // next alternative ( | )
        public int Min, Max;      // repetition (Max < 0 = unbounded)
    }

    /// <summary>A continuation: what still has to match after the current node.
    /// A linked structure (field references, never an object[] element).</summary>
    public sealed class BzRxCont
    {
        public BzRxNode Node;
        public BzRxCont Next;
    }

    /// <summary>A small backtracking regular-expression engine
    /// (System.Text.RegularExpressions). Supported syntax: literals, `.`,
    /// character classes `[abc]` / `[^a-z]` with ranges, anchors `^` and `$`,
    /// quantifiers `*` `+` `?`, alternation `|`, groups `(...)`, and the escapes
    /// `\d \D \w \W \s \S` plus `\` before any metacharacter.
    /// Not supported: backreferences, lazy quantifiers, `{n,m}`, lookaround,
    /// capture extraction (groups only affect grouping).</summary>
    public sealed class BzRegex
    {
        BzRxNode _root;
        char[] _pat;
        int _plen;
        int _pos;

        public BzRegex(string pattern)
        {
            _pat = new char[pattern.Length];
            for (int i = 0; i < pattern.Length; i++) _pat[i] = pattern[i];
            _plen = pattern.Length;
            _pos = 0;
            _root = ParseAlt();
        }

        public BzRegex(char[] pattern, int len)
        {
            _pat = pattern; _plen = len; _pos = 0;
            _root = ParseAlt();
        }

        // ---- parsing --------------------------------------------------------
        // alt  := seq ('|' seq)*
        // seq  := rep*
        // rep  := atom ('*' | '+' | '?')?
        // atom := '(' alt ')' | '[' class ']' | '.' | '^' | '$' | '\' esc | char

        BzRxNode ParseAlt()
        {
            BzRxNode first = ParseSeq();
            BzRxNode tail = first;
            while (_pos < _plen && _pat[_pos] == '|')
            {
                _pos++;
                BzRxNode next = ParseSeq();
                tail.Alt = next;
                tail = next;
            }
            return first;
        }

        BzRxNode ParseSeq()
        {
            BzRxNode head = null, tail = null;
            while (_pos < _plen && _pat[_pos] != '|' && _pat[_pos] != ')')
            {
                BzRxNode n = ParseRep();
                if (n == null) break;
                if (head == null) { head = n; tail = n; }
                else { tail.Next = n; tail = n; }
            }
            if (head == null)
            {
                // An empty branch matches the empty string: use a zero-width group.
                head = new BzRxNode();
                head.Kind = BzRxNode.GROUP;
                head.Min = 0; head.Max = 0;
            }
            return head;
        }

        BzRxNode ParseRep()
        {
            BzRxNode a = ParseAtom();
            if (a == null) return null;
            a.Min = 1; a.Max = 1;
            if (_pos < _plen)
            {
                char c = _pat[_pos];
                if (c == '*') { a.Min = 0; a.Max = -1; _pos++; }
                else if (c == '+') { a.Min = 1; a.Max = -1; _pos++; }
                else if (c == '?') { a.Min = 0; a.Max = 1; _pos++; }
            }
            return a;
        }

        BzRxNode ParseAtom()
        {
            if (_pos >= _plen) return null;
            char c = _pat[_pos++];
            BzRxNode n = new BzRxNode();
            if (c == '(')
            {
                n.Kind = BzRxNode.GROUP;
                n.Sub = ParseAlt();
                if (_pos < _plen && _pat[_pos] == ')') _pos++;
                return n;
            }
            if (c == '[')
            {
                n.Kind = BzRxNode.CLASS;
                if (_pos < _plen && _pat[_pos] == '^') { n.Negate = true; _pos++; }
                char[] set = new char[64];
                int sl = 0;
                while (_pos < _plen && _pat[_pos] != ']')
                {
                    char a = _pat[_pos++];
                    if (a == '\\' && _pos < _plen) a = Unescape(_pat[_pos++], set, ref sl);
                    else if (_pos + 1 < _plen && _pat[_pos] == '-' && _pat[_pos + 1] != ']')
                    {
                        char hi = _pat[_pos + 1];
                        _pos += 2;
                        for (char x = a; x <= hi && sl < set.Length - 1; x++) set[sl++] = x;
                        continue;
                    }
                    if (a != '\0' && sl < set.Length) set[sl++] = a;
                }
                if (_pos < _plen && _pat[_pos] == ']') _pos++;
                n.Set = set; n.SetLen = sl;
                return n;
            }
            if (c == '.') { n.Kind = BzRxNode.ANY; return n; }
            if (c == '^') { n.Kind = BzRxNode.BOL; return n; }
            if (c == '$') { n.Kind = BzRxNode.EOL; return n; }
            if (c == '\\' && _pos < _plen)
            {
                char e = _pat[_pos++];
                char[] set = new char[128];
                int sl = 0;
                char lit = Unescape(e, set, ref sl);
                if (sl > 0)
                {
                    n.Kind = BzRxNode.CLASS;
                    n.Set = set; n.SetLen = sl;
                    n.Negate = (e == 'D' || e == 'W' || e == 'S');
                    return n;
                }
                n.Kind = BzRxNode.CHAR; n.Ch = lit;
                return n;
            }
            n.Kind = BzRxNode.CHAR; n.Ch = c;
            return n;
        }

        // Expand \d \w \s (and their negations) into `set`; otherwise return the
        // literal character the escape stands for.
        static char Unescape(char e, char[] set, ref int sl)
        {
            if (e == 'd' || e == 'D')
            {
                for (char x = '0'; x <= '9'; x++) set[sl++] = x;
                return '\0';
            }
            if (e == 'w' || e == 'W')
            {
                for (char x = 'a'; x <= 'z'; x++) set[sl++] = x;
                for (char x = 'A'; x <= 'Z'; x++) set[sl++] = x;
                for (char x = '0'; x <= '9'; x++) set[sl++] = x;
                set[sl++] = '_';
                return '\0';
            }
            if (e == 's' || e == 'S')
            {
                set[sl++] = ' '; set[sl++] = '\t'; set[sl++] = '\n'; set[sl++] = '\r';
                return '\0';
            }
            if (e == 'n') return '\n';
            if (e == 't') return '\t';
            if (e == 'r') return '\r';
            return e;
        }

        // ---- matching -------------------------------------------------------

        bool Single(BzRxNode n, char[] s, int len, int i)
        {
            if (i >= len) return false;
            char c = s[i];
            if (n.Kind == BzRxNode.CHAR) return c == n.Ch;
            if (n.Kind == BzRxNode.ANY) return true;
            if (n.Kind == BzRxNode.CLASS)
            {
                bool hit = false;
                for (int k = 0; k < n.SetLen; k++) if (n.Set[k] == c) { hit = true; break; }
                return n.Negate ? !hit : hit;
            }
            return false;
        }

        // Match `n` (and its Next chain) at s[i..], then the continuation stack
        // `k`. Returns the end index, or -1. Keeping the continuation explicit is
        // what makes backtracking correct across a group boundary: `(a|ab)c`
        // must be able to retry the second alternative after `c` fails.
        int MatchHere(BzRxNode n, BzRxCont k, char[] s, int len, int i)
        {
            if (n == null)
            {
                if (k == null) return i;
                return MatchHere(k.Node, k.Next, s, len, i);
            }

            if (n.Kind == BzRxNode.BOL)
                return i == 0 ? MatchHere(n.Next, k, s, len, i) : -1;
            if (n.Kind == BzRxNode.EOL)
                return i == len ? MatchHere(n.Next, k, s, len, i) : -1;

            if (n.Kind == BzRxNode.GROUP)
            {
                if (n.Max == 0) return MatchHere(n.Next, k, s, len, i);   // empty branch
                if (n.Min == 1 && n.Max == 1)
                {
                    BzRxCont after = new BzRxCont();
                    after.Node = n.Next; after.Next = k;
                    return MatchAlt(n.Sub, after, s, len, i);
                }
                // A quantified group: try the longest run first (greedy), then
                // shorter ones. Each iteration is matched without a continuation,
                // which is the usual simplification for `(...)*`.
                int cap = len - i + 2;
                int[] ends = new int[cap];
                int count = 0;
                ends[0] = i;
                int cur = i;
                while ((n.Max < 0 || count < n.Max) && count < cap - 1)
                {
                    int e = MatchAlt(n.Sub, null, s, len, cur);
                    if (e < 0 || e == cur) break;      // no progress -> stop
                    cur = e; count++; ends[count] = cur;
                }
                for (int c = count; c >= n.Min; c--)
                {
                    int r = MatchHere(n.Next, k, s, len, ends[c]);
                    if (r >= 0) return r;
                }
                return -1;
            }

            // CHAR / ANY / CLASS, with or without a quantifier.
            if (n.Min == 1 && n.Max == 1)
                return Single(n, s, len, i) ? MatchHere(n.Next, k, s, len, i + 1) : -1;

            int max = i;
            while ((n.Max < 0 || max - i < n.Max) && Single(n, s, len, max)) max++;
            for (int j = max; j >= i + n.Min; j--)
            {
                int r = MatchHere(n.Next, k, s, len, j);
                if (r >= 0) return r;
            }
            return -1;
        }

        // Try each alternative branch of `alt` in order, continuing with `k`.
        int MatchAlt(BzRxNode alt, BzRxCont k, char[] s, int len, int i)
        {
            BzRxNode a = alt;
            while (a != null)
            {
                int r = MatchHere(a, k, s, len, i);
                if (r >= 0) return r;
                a = a.Alt;
            }
            return -1;
        }

        /// <summary>Match anchored at `start`; returns the end index or -1.</summary>
        public int MatchAt(char[] s, int len, int start) => MatchAlt(_root, null, s, len, start);

        /// <summary>True if the pattern matches anywhere in `s[0..len]`.</summary>
        public bool IsMatch(char[] s, int len)
        {
            for (int i = 0; i <= len; i++) if (MatchAt(s, len, i) >= 0) return true;
            return false;
        }

        public bool IsMatch(string s)
        {
            char[] t = new char[s.Length];
            for (int i = 0; i < s.Length; i++) t[i] = s[i];
            return IsMatch(t, s.Length);
        }

        /// <summary>Find the first match. Returns its start index (-1 if none)
        /// and writes the end index into `end`.</summary>
        public int Match(char[] s, int len, out int end)
        {
            for (int i = 0; i <= len; i++)
            {
                int e = MatchAt(s, len, i);
                if (e >= 0) { end = e; return i; }
            }
            end = -1;
            return -1;
        }

        /// <summary>Replace every non-overlapping match with `rep`; writes into
        /// `dst` and returns the new length (-1 if `dst` is too small).</summary>
        public int Replace(char[] s, int len, char[] rep, int repLen, char[] dst)
        {
            int n = 0, i = 0;
            while (i <= len)
            {
                int e = MatchAt(s, len, i);
                if (e >= 0 && e > i)
                {
                    if (n + repLen > dst.Length) return -1;
                    for (int k = 0; k < repLen; k++) dst[n++] = rep[k];
                    i = e;
                }
                else
                {
                    if (i >= len) break;
                    if (n + 1 > dst.Length) return -1;
                    dst[n++] = s[i++];
                }
            }
            return n;
        }

        /// <summary>Split on matches. Returns the pieces as a list of char[]
        /// (a BzRefList, since an object[] of references would fault).</summary>
        public BzRefList<char[]> Split(char[] s, int len)
        {
            BzRefList<char[]> parts = new BzRefList<char[]>();
            int start = 0, i = 0;
            while (i < len)
            {
                int e = MatchAt(s, len, i);
                if (e > i)
                {
                    char[] piece = new char[i - start];
                    for (int k = 0; k < i - start; k++) piece[k] = s[start + k];
                    parts.Add(piece);
                    start = e; i = e;
                }
                else i++;
            }
            char[] last = new char[len - start];
            for (int k = 0; k < len - start; k++) last[k] = s[start + k];
            parts.Add(last);
            return parts;
        }
    }

    // =====================================================================
    // System.Globalization
    // =====================================================================

    /// <summary>Number and date formatting (System.Globalization). Output goes
    /// into a caller-supplied char[]; every method returns the length written.</summary>
    public static class BzCulture
    {
        /// <summary>Format an integer into `dst` starting at `off`, optionally with
        /// a thousands separator. Returns the new end offset.</summary>
        public static int FormatIntAt(long v, char[] dst, int off, bool group, char sep)
        {
            int n = off;
            bool neg = v < 0;
            ulong x = neg ? (ulong)(-v) : (ulong)v;
            char[] tmp = new char[24];
            int t = 0;
            if (x == 0) tmp[t++] = '0';
            while (x > 0) { tmp[t++] = (char)('0' + (int)(x % 10)); x /= 10; }
            if (neg && n < dst.Length) dst[n++] = '-';
            for (int i = t - 1; i >= 0; i--)
            {
                if (n >= dst.Length) return n;
                dst[n++] = tmp[i];
                if (group && i > 0 && i % 3 == 0 && n < dst.Length) dst[n++] = sep;
            }
            return n;
        }

        public static int FormatInt(long v, char[] dst, bool group, char sep) => FormatIntAt(v, dst, 0, group, sep);
        public static int FormatInt(long v, char[] dst) => FormatIntAt(v, dst, 0, false, ',');
        public static int FormatGrouped(long v, char[] dst) => FormatIntAt(v, dst, 0, true, ',');

        /// <summary>Format `value / 10^decimals` as a fixed-point decimal
        /// (there is no floating point in these apps).</summary>
        public static int FormatFixed(long value, int decimals, char[] dst, char point)
        {
            if (decimals <= 0) return FormatInt(value, dst);
            long scale = 1;
            for (int i = 0; i < decimals; i++) scale *= 10;
            bool neg = value < 0;
            long a = neg ? -value : value;
            long ip = a / scale, fp = a % scale;
            int n = 0;
            if (neg && n < dst.Length) dst[n++] = '-';
            n = FormatIntAt(ip, dst, n, false, ',');
            if (n < dst.Length) dst[n++] = point;
            for (int d = decimals - 1; d >= 0; d--)
            {
                long div = 1;
                for (int i = 0; i < d; i++) div *= 10;
                if (n >= dst.Length) break;
                dst[n++] = (char)('0' + (int)((fp / div) % 10));
            }
            return n;
        }

        /// <summary>Format a percentage (0..100) as "NN%".</summary>
        public static int FormatPercent(int pct, char[] dst)
        {
            int n = FormatInt(pct, dst);
            if (n < dst.Length) dst[n++] = '%';
            return n;
        }

        /// <summary>Format bytes as a human-readable size (B / KiB / MiB / GiB),
        /// with one decimal place above 1 KiB.</summary>
        public static int FormatBytes(ulong bytes, char[] dst)
        {
            const string U = "BKMG";
            int unit = 0;
            ulong v = bytes, rem = 0;
            while (v >= 1024 && unit < 3) { rem = v % 1024; v /= 1024; unit++; }
            int n = FormatInt((long)v, dst);
            if (unit > 0 && n + 2 <= dst.Length)
            {
                dst[n++] = '.';
                dst[n++] = (char)('0' + (int)(rem * 10 / 1024));   // truncated, not rounded
            }
            if (n < dst.Length) dst[n++] = U[unit];
            if (unit > 0 && n + 2 <= dst.Length) { dst[n++] = 'i'; dst[n++] = 'B'; }
            return n;
        }

        /// <summary>Invariant upper-casing of ASCII letters, in place.</summary>
        public static void ToUpperInvariant(char[] s, int len)
        {
            for (int i = 0; i < len; i++) if (s[i] >= 'a' && s[i] <= 'z') s[i] = (char)(s[i] - 32);
        }

        public static void ToLowerInvariant(char[] s, int len)
        {
            for (int i = 0; i < len; i++) if (s[i] >= 'A' && s[i] <= 'Z') s[i] = (char)(s[i] + 32);
        }

        /// <summary>Three-letter English month abbreviation (month is 1..12).</summary>
        public static int MonthAbbrev(int month, char[] dst)
        {
            const string M = "JanFebMarAprMayJunJulAugSepOctNovDec";
            if (month < 1 || month > 12 || dst.Length < 3) return 0;
            int b = (month - 1) * 3;
            dst[0] = M[b]; dst[1] = M[b + 1]; dst[2] = M[b + 2];
            return 3;
        }
    }

    /// <summary>Wall-clock date and time (System.DateTime) read from the CMOS
    /// real-time clock via the CLOCK_RTC syscall. Local time as the firmware
    /// reports it; there is no timezone database.</summary>
    public sealed class BzDateTime
    {
        [DllImport("*")] static extern unsafe ulong bz_clock_rtc(ulong* outp);

        public int Year, Month, Day, Hour, Minute, Second;

        /// <summary>Read the current date and time.</summary>
        public static unsafe BzDateTime Now()
        {
            ulong* t = stackalloc ulong[6];
            for (int i = 0; i < 6; i++) t[i] = 0;
            bz_clock_rtc(t);
            BzDateTime d = new BzDateTime();
            d.Year = (int)t[0]; d.Month = (int)t[1]; d.Day = (int)t[2];
            d.Hour = (int)t[3]; d.Minute = (int)t[4]; d.Second = (int)t[5];
            return d;
        }

        /// <summary>True when the reading looks like a real calendar date.</summary>
        public bool IsValid =>
            Year >= 1970 && Year <= 2200 &&
            Month >= 1 && Month <= 12 &&
            Day >= 1 && Day <= 31 &&
            Hour <= 23 && Minute <= 59 && Second <= 59;

        public static bool IsLeapYear(int y) => (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;

        public static int DaysInMonth(int year, int month)
        {
            if (month == 2) return IsLeapYear(year) ? 29 : 28;
            if (month == 4 || month == 6 || month == 9 || month == 11) return 30;
            return 31;
        }

        /// <summary>Day of week, 0 = Sunday (Sakamoto's algorithm).</summary>
        public int DayOfWeek()
        {
            int y = Year, m = Month, d = Day;
            int[] t = new int[] { 0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4 };
            if (m < 3) y -= 1;
            return (y + y / 4 - y / 100 + y / 400 + t[m - 1] + d) % 7;
        }

        static int Two(int v, char[] dst, int n)
        {
            if (n + 2 > dst.Length) return n;
            dst[n++] = (char)('0' + (v / 10) % 10);
            dst[n++] = (char)('0' + v % 10);
            return n;
        }

        /// <summary>Format as HH:MM:SS. Returns the length written.</summary>
        public int FormatTime(char[] dst)
        {
            int n = Two(Hour, dst, 0);
            if (n < dst.Length) dst[n++] = ':';
            n = Two(Minute, dst, n);
            if (n < dst.Length) dst[n++] = ':';
            return Two(Second, dst, n);
        }

        /// <summary>Format as YYYY-MM-DD. Returns the length written.</summary>
        public int FormatDate(char[] dst)
        {
            int n = BzCulture.FormatIntAt(Year, dst, 0, false, ',');
            if (n < dst.Length) dst[n++] = '-';
            n = Two(Month, dst, n);
            if (n < dst.Length) dst[n++] = '-';
            return Two(Day, dst, n);
        }

        /// <summary>Format as YYYY-MM-DD HH:MM:SS.</summary>
        public int Format(char[] dst)
        {
            int n = FormatDate(dst);
            if (n < dst.Length) dst[n++] = ' ';
            char[] t = new char[8];
            int tn = FormatTime(t);
            for (int i = 0; i < tn && n < dst.Length; i++) dst[n++] = t[i];
            return n;
        }
    }

    // =====================================================================
    // System.Diagnostics
    // =====================================================================

    /// <summary>A high-resolution elapsed-time counter (System.Diagnostics.Stopwatch)
    /// over the CLOCK_MONO syscall, which returns the CPU time-stamp counter.
    /// The TSC frequency is not exposed, so raw ticks are the honest unit — use
    /// <see cref="BzSystemInfo"/>'s uptime for time in seconds.</summary>
    public sealed class BzStopwatch
    {
        [DllImport("*")] static extern ulong bz_clock_mono();

        ulong _start, _elapsed;
        bool _running;

        public static BzStopwatch StartNew() { BzStopwatch w = new BzStopwatch(); w.Start(); return w; }

        public void Start() { if (!_running) { _start = bz_clock_mono(); _running = true; } }
        public void Stop() { if (_running) { _elapsed += bz_clock_mono() - _start; _running = false; } }
        public void Reset() { _elapsed = 0; _running = false; }
        public void Restart() { _elapsed = 0; _start = bz_clock_mono(); _running = true; }
        public bool IsRunning => _running;

        /// <summary>Elapsed CPU time-stamp counter ticks.</summary>
        public ulong ElapsedTicks => _running ? _elapsed + (bz_clock_mono() - _start) : _elapsed;

        /// <summary>Raw monotonic counter reading.</summary>
        public static ulong Timestamp() => bz_clock_mono();
    }

    /// <summary>One entry from the kernel process table.</summary>
    public sealed class BzProcessInfo
    {
        public ulong Pid;
        public ulong State;      // see abi::proc_state
        public ulong CpuTicks;
        public ulong Kind;       // 0 = kernel task, 1 = user app
        public char[] Name;      // NUL-trimmed
        public int NameLen;
        public BzProcessInfo Next;
    }

    /// <summary>Process inspection and control (System.Diagnostics.Process) over
    /// the PROC_LIST / PROC_KILL syscalls.</summary>
    public static class BzProcess
    {
        [DllImport("*")] static extern unsafe ulong bz_proc_list(byte* buf, ulong max);
        [DllImport("*")] static extern ulong bz_proc_kill(ulong pid);

        const int PROC_SIZE = 64;   // abi::ProcInfo
        const int NAME_OFF = 32;
        const int NAME_MAX = 32;

        /// <summary>Snapshot of the process table, newest kernel order preserved.</summary>
        public static unsafe BzProcessInfo GetProcesses(int max)
        {
            if (max <= 0) max = 32;
            byte[] buf = new byte[PROC_SIZE * max];
            ulong n;
            fixed (byte* p = buf) n = bz_proc_list(p, (ulong)max);
            BzProcessInfo head = null, tail = null;
            for (ulong i = 0; i < n; i++)
            {
                int b = (int)i * PROC_SIZE;
                BzProcessInfo e = new BzProcessInfo();
                e.Pid = ReadU64(buf, b + 0);
                e.State = ReadU64(buf, b + 8);
                e.CpuTicks = ReadU64(buf, b + 16);
                e.Kind = ReadU64(buf, b + 24);
                char[] nm = new char[NAME_MAX];
                int nl = 0;
                for (int k = 0; k < NAME_MAX; k++)
                {
                    byte c = buf[b + NAME_OFF + k];
                    if (c == 0) break;
                    nm[nl++] = (char)c;
                }
                e.Name = nm; e.NameLen = nl;
                if (head == null) { head = e; tail = e; } else { tail.Next = e; tail = e; }
            }
            return head;
        }

        public static int Count(BzProcessInfo head)
        {
            int n = 0;
            BzProcessInfo p = head;
            while (p != null) { n++; p = p.Next; }
            return n;
        }

        /// <summary>Find a process by name (case-sensitive); null if absent.</summary>
        public static BzProcessInfo FindByName(BzProcessInfo head, string name)
        {
            BzProcessInfo p = head;
            while (p != null)
            {
                if (p.NameLen == name.Length)
                {
                    bool same = true;
                    for (int i = 0; i < p.NameLen; i++) if (p.Name[i] != name[i]) { same = false; break; }
                    if (same) return p;
                }
                p = p.Next;
            }
            return null;
        }

        /// <summary>Terminate a process. Returns true on success.</summary>
        public static bool Kill(ulong pid) => bz_proc_kill(pid) == 0;

        internal static ulong ReadU64(byte[] b, int off)
        {
            ulong v = 0;
            for (int i = 7; i >= 0; i--) v = (v << 8) | b[off + i];
            return v;
        }
    }

    /// <summary>Trace output and assertions (System.Diagnostics.Debug), routed to
    /// the kernel serial log.</summary>
    public static class BzDebug
    {
        [DllImport("*")] static extern unsafe void bz_write(byte* buf, ulong len);

        public static unsafe void Write(string s)
        {
            byte* tmp = stackalloc byte[256];
            int n = 0;
            for (int i = 0; i < s.Length && n < 256; i++) tmp[n++] = (byte)s[i];
            bz_write(tmp, (ulong)n);
        }

        public static void WriteLine(string s) { Write(s); Write("\n"); }

        public static void Write(char[] s, int len) => Con.Write(s, len);
        public static void WriteLine(char[] s, int len) { Con.Write(s, len); Write("\n"); }

        /// <summary>Print a message when `condition` is false. Returns `condition`
        /// so callers can accumulate a pass/fail result.</summary>
        public static bool Assert(bool condition, string message)
        {
            if (!condition) { Write("ASSERT FAILED: "); WriteLine(message); }
            return condition;
        }
    }

    // =====================================================================
    // System.Management
    // =====================================================================

    /// <summary>Machine and subsystem information (System.Management), read from
    /// the SYS_STAT / AUDIO_STAT / PKG_LIST syscalls.</summary>
    public sealed class BzSystemInfo
    {
        [DllImport("*")] static extern unsafe ulong bz_sys_stat(byte* outp);
        [DllImport("*")] static extern unsafe ulong bz_audio_stat(byte* outp);

        public ulong UptimeTicks, TickHz, HeapUsed, HeapTotal, TaskCount, MemTotalMib;
        public bool AudioPresent;
        public ulong AudioSampleRate, AudioChannels, AudioBits, AudioVolume;
        public bool AudioMuted;

        /// <summary>Read the current machine state.</summary>
        public static unsafe BzSystemInfo Query()
        {
            BzSystemInfo s = new BzSystemInfo();
            byte[] buf = new byte[48];
            fixed (byte* p = buf) bz_sys_stat(p);
            s.UptimeTicks = BzProcess.ReadU64(buf, 0);
            s.TickHz = BzProcess.ReadU64(buf, 8);
            s.HeapUsed = BzProcess.ReadU64(buf, 16);
            s.HeapTotal = BzProcess.ReadU64(buf, 24);
            s.TaskCount = BzProcess.ReadU64(buf, 32);
            s.MemTotalMib = BzProcess.ReadU64(buf, 40);

            byte[] a = new byte[48];
            fixed (byte* p = a) bz_audio_stat(p);
            s.AudioPresent = BzProcess.ReadU64(a, 0) != 0;
            s.AudioSampleRate = BzProcess.ReadU64(a, 8);
            s.AudioChannels = BzProcess.ReadU64(a, 16);
            s.AudioBits = BzProcess.ReadU64(a, 24);
            s.AudioVolume = BzProcess.ReadU64(a, 32);
            s.AudioMuted = BzProcess.ReadU64(a, 40) != 0;
            return s;
        }

        /// <summary>Uptime in whole seconds (0 if the kernel reports no tick rate).</summary>
        public ulong UptimeSeconds => TickHz == 0 ? 0 : UptimeTicks / TickHz;

        /// <summary>Kernel heap use as a percentage, 0..100.</summary>
        public int HeapPercent => HeapTotal == 0 ? 0 : (int)(HeapUsed * 100 / HeapTotal);
    }

    // =====================================================================
    // GC (System)
    // =====================================================================

    /// <summary>Managed-heap statistics (System.GC). The ring-3 heap is a growable
    /// bump allocator, so <see cref="Collect"/> reclaims nothing — it is present so
    /// code written against it keeps working once a real collector lands. The
    /// numbers below are real measurements from the allocator, not estimates.</summary>
    public static class BzGC
    {
        [DllImport("*")] static extern unsafe void bz_heap_stats(ulong* outp);

        /// <summary>Total bytes handed out by the allocator since start-up.</summary>
        public static unsafe ulong GetAllocatedBytes()
        {
            ulong* s = stackalloc ulong[5];
            bz_heap_stats(s);
            return s[0];
        }

        /// <summary>Bytes currently mapped for the managed heap.</summary>
        public static unsafe ulong GetTotalMemory()
        {
            ulong* s = stackalloc ulong[5];
            bz_heap_stats(s);
            return s[1];
        }

        /// <summary>Number of heap chunks mapped so far (each is at least 1 MiB).</summary>
        public static unsafe ulong ChunkCount()
        {
            ulong* s = stackalloc ulong[5];
            bz_heap_stats(s);
            return s[2];
        }

        /// <summary>Number of allocations served.</summary>
        public static unsafe ulong AllocationCount()
        {
            ulong* s = stackalloc ulong[5];
            bz_heap_stats(s);
            return s[3];
        }

        /// <summary>Bytes still free in the current chunk before it must grow.</summary>
        public static unsafe ulong FreeInChunk()
        {
            ulong* s = stackalloc ulong[5];
            bz_heap_stats(s);
            return s[4];
        }

        /// <summary>No-op: the bump heap has no reclaiming collector yet. Returns
        /// false so callers can tell that nothing was collected.</summary>
        public static bool Collect() => false;

        /// <summary>Always 0 — there are no generations.</summary>
        public static int MaxGeneration => 0;
    }

    // =====================================================================
    // Pkg — the package-manager API
    // =====================================================================

    /// <summary>One registry entry from the kernel package manager.</summary>
    public sealed class BzPkgInfo
    {
        public char[] Name;
        public int NameLen;
        public char[] Category;
        public int CategoryLen;
        public bool Installed;
        public byte[] NameBytes;   // the exact bytes PKG_SET expects
        public BzPkgInfo Next;
    }

    /// <summary>The package manager (`bz install` / the App Store) as a library,
    /// over the PKG_LIST / PKG_SET syscalls.</summary>
    public static class BzPkg
    {
        [DllImport("*")] static extern unsafe ulong bz_pkg_list(byte* buf, ulong max);
        [DllImport("*")] static extern unsafe ulong bz_pkg_set(byte* name, ulong len, ulong action);

        const int PKG_SIZE = 48;    // abi::PkgInfo
        const int NAME_MAX = 24;
        const int CAT_OFF = 24;
        const int CAT_MAX = 16;
        const int INSTALLED_OFF = 40;

        /// <summary>Read the whole registry.</summary>
        public static unsafe BzPkgInfo List(int max)
        {
            if (max <= 0) max = 32;
            byte[] buf = new byte[PKG_SIZE * max];
            ulong n;
            fixed (byte* p = buf) n = bz_pkg_list(p, (ulong)max);
            BzPkgInfo head = null, tail = null;
            for (ulong i = 0; i < n; i++)
            {
                int b = (int)i * PKG_SIZE;
                BzPkgInfo e = new BzPkgInfo();
                char[] nm = new char[NAME_MAX];
                byte[] raw = new byte[NAME_MAX];
                int nl = 0;
                for (int k = 0; k < NAME_MAX; k++)
                {
                    byte c = buf[b + k];
                    if (c == 0) break;
                    nm[nl] = (char)c; raw[nl] = c; nl++;
                }
                char[] ct = new char[CAT_MAX];
                int cl = 0;
                for (int k = 0; k < CAT_MAX; k++)
                {
                    byte c = buf[b + CAT_OFF + k];
                    if (c == 0) break;
                    ct[cl++] = (char)c;
                }
                e.Name = nm; e.NameLen = nl; e.NameBytes = raw;
                e.Category = ct; e.CategoryLen = cl;
                e.Installed = BzProcess.ReadU64(buf, b + INSTALLED_OFF) != 0;
                if (head == null) { head = e; tail = e; } else { tail.Next = e; tail = e; }
            }
            return head;
        }

        public static int Count(BzPkgInfo head)
        {
            int n = 0;
            BzPkgInfo p = head;
            while (p != null) { n++; p = p.Next; }
            return n;
        }

        /// <summary>Find a package by name; null if it is not in the registry.</summary>
        public static BzPkgInfo Find(BzPkgInfo head, string name)
        {
            BzPkgInfo p = head;
            while (p != null)
            {
                if (p.NameLen == name.Length)
                {
                    bool same = true;
                    for (int i = 0; i < p.NameLen; i++) if (p.Name[i] != name[i]) { same = false; break; }
                    if (same) return p;
                }
                p = p.Next;
            }
            return null;
        }

        /// <summary>Packages whose name contains `term` (a simple substring search).</summary>
        public static BzRefList<BzPkgInfo> Search(BzPkgInfo head, string term)
        {
            BzRefList<BzPkgInfo> hits = new BzRefList<BzPkgInfo>();
            BzPkgInfo p = head;
            while (p != null)
            {
                for (int i = 0; i + term.Length <= p.NameLen; i++)
                {
                    bool same = true;
                    for (int k = 0; k < term.Length; k++) if (p.Name[i + k] != term[k]) { same = false; break; }
                    if (same) { hits.Add(p); break; }
                }
                p = p.Next;
            }
            return hits;
        }

        public static bool IsInstalled(BzPkgInfo head, string name)
        {
            BzPkgInfo p = Find(head, name);
            return p != null && p.Installed;
        }

        static unsafe bool Set(BzPkgInfo pkg, ulong action)
        {
            if (pkg == null) return false;
            fixed (byte* p = pkg.NameBytes) return bz_pkg_set(p, (ulong)pkg.NameLen, action) == 0;
        }

        /// <summary>Install a package (the shell's `run` is gated on this).</summary>
        public static bool Install(BzPkgInfo pkg) => Set(pkg, 1);

        /// <summary>Remove a package.</summary>
        public static bool Remove(BzPkgInfo pkg) => Set(pkg, 0);
    }

    // =====================================================================
    // System.IO
    // =====================================================================

    /// <summary>Path manipulation (System.IO.Path). Buitenzorg paths look like
    /// `/disk/PHOTO.BMP`: a leading mount segment then FAT 8.3 names. Everything
    /// works on char[] buffers because zerolib cannot build strings.</summary>
    public static class BzPath
    {
        public const char Separator = '/';

        /// <summary>Join `a` and `b` with a single separator into `dst`;
        /// returns the length written.</summary>
        public static int Combine(char[] a, int an, char[] b, int bn, char[] dst)
        {
            int n = 0;
            for (int i = 0; i < an && n < dst.Length; i++) dst[n++] = a[i];
            if (n > 0 && dst[n - 1] != Separator && bn > 0 && b[0] != Separator && n < dst.Length)
                dst[n++] = Separator;
            int start = (n > 0 && dst[n - 1] == Separator && bn > 0 && b[0] == Separator) ? 1 : 0;
            for (int i = start; i < bn && n < dst.Length; i++) dst[n++] = b[i];
            return n;
        }

        public static int Combine(string a, char[] b, int bn, char[] dst)
        {
            char[] t = new char[a.Length];
            for (int i = 0; i < a.Length; i++) t[i] = a[i];
            return Combine(t, a.Length, b, bn, dst);
        }

        /// <summary>Index just past the last separator (0 if there is none).</summary>
        public static int FileNameStart(char[] p, int len)
        {
            for (int i = len - 1; i >= 0; i--) if (p[i] == Separator) return i + 1;
            return 0;
        }

        /// <summary>Copy the file-name part into `dst`; returns its length.</summary>
        public static int GetFileName(char[] p, int len, char[] dst)
        {
            int s = FileNameStart(p, len);
            int n = 0;
            for (int i = s; i < len && n < dst.Length; i++) dst[n++] = p[i];
            return n;
        }

        /// <summary>Copy the directory part (without a trailing separator).</summary>
        public static int GetDirectoryName(char[] p, int len, char[] dst)
        {
            int s = FileNameStart(p, len);
            int end = s > 0 ? s - 1 : 0;
            int n = 0;
            for (int i = 0; i < end && n < dst.Length; i++) dst[n++] = p[i];
            return n;
        }

        /// <summary>Copy the extension including the dot ("" if there is none).</summary>
        public static int GetExtension(char[] p, int len, char[] dst)
        {
            int s = FileNameStart(p, len);
            int dot = -1;
            for (int i = len - 1; i >= s; i--) if (p[i] == '.') { dot = i; break; }
            if (dot < 0) return 0;
            int n = 0;
            for (int i = dot; i < len && n < dst.Length; i++) dst[n++] = p[i];
            return n;
        }

        /// <summary>True if the name ends with `ext` (case-insensitive ASCII).</summary>
        public static bool HasExtension(char[] p, int len, string ext)
        {
            if (len < ext.Length) return false;
            for (int i = 0; i < ext.Length; i++)
            {
                char a = p[len - ext.Length + i], b = ext[i];
                if (a >= 'a' && a <= 'z') a = (char)(a - 32);
                if (b >= 'a' && b <= 'z') b = (char)(b - 32);
                if (a != b) return false;
            }
            return true;
        }

        /// <summary>Drop a trailing "/name" segment (navigate up). Returns the new
        /// length; a path with no separator collapses to 0 (the mount list).</summary>
        public static int Up(char[] p, int len)
        {
            for (int i = len - 1; i > 0; i--) if (p[i] == Separator) return i;
            return 0;
        }
    }

    /// <summary>File reading and writing (System.IO.File) over the FS_READ and
    /// FS_WRITE syscalls. Reads work on any mount the kernel can see; writes need
    /// a writable mount (the FAT12 RAM disk at `/ram`).</summary>
    public static class BzFile
    {
        [DllImport("*")] static extern unsafe ulong bz_fs_read(byte* path, byte* buf, ulong max);
        [DllImport("*")] static extern unsafe ulong bz_fs_write(byte* path, byte* buf, ulong len);

        /// <summary>Copy a path into a NUL-terminated byte buffer for the syscall.</summary>
        internal static unsafe void PathBytes(char[] path, int len, byte* dst, int cap)
        {
            int n = 0;
            for (int i = 0; i < len && n < cap - 1; i++) dst[n++] = (byte)path[i];
            dst[n] = 0;
        }

        internal static unsafe void PathBytes(string path, byte* dst, int cap)
        {
            int n = 0;
            for (int i = 0; i < path.Length && n < cap - 1; i++) dst[n++] = (byte)path[i];
            dst[n] = 0;
        }

        /// <summary>Read a file. Returns the bytes read; `data` receives the buffer
        /// (allocated at `max` bytes) or null if the read failed.</summary>
        public static unsafe int ReadAllBytes(char[] path, int plen, int max, out byte[] data)
        {
            byte* p = stackalloc byte[256];
            PathBytes(path, plen, p, 256);
            byte[] buf = new byte[max];
            ulong n;
            fixed (byte* b = buf) n = bz_fs_read(p, b, (ulong)max);
            if (n == 0) { data = null; return 0; }
            data = buf;
            return (int)n;
        }

        public static unsafe int ReadAllBytes(string path, int max, out byte[] data)
        {
            char[] t = new char[path.Length];
            for (int i = 0; i < path.Length; i++) t[i] = path[i];
            return ReadAllBytes(t, path.Length, max, out data);
        }

        /// <summary>Read a text file into a char[] (ASCII). Returns the length.</summary>
        public static int ReadAllChars(string path, int max, out char[] text)
        {
            byte[] data;
            int n = ReadAllBytes(path, max, out data);
            if (n == 0) { text = null; return 0; }
            char[] t = new char[n];
            for (int i = 0; i < n; i++) t[i] = (char)data[i];
            text = t;
            return n;
        }

        /// <summary>Write bytes to a file (creating or truncating it). Returns the
        /// number of bytes written; 0 means the mount is read-only or full.</summary>
        public static unsafe int WriteAllBytes(char[] path, int plen, byte[] data, int len)
        {
            byte* p = stackalloc byte[256];
            PathBytes(path, plen, p, 256);
            ulong n;
            fixed (byte* b = data) n = bz_fs_write(p, b, (ulong)len);
            return (int)n;
        }

        public static unsafe int WriteAllBytes(string path, byte[] data, int len)
        {
            char[] t = new char[path.Length];
            for (int i = 0; i < path.Length; i++) t[i] = path[i];
            return WriteAllBytes(t, path.Length, data, len);
        }

        /// <summary>Write ASCII text to a file. Returns the byte count written.</summary>
        public static int WriteAllChars(string path, char[] text, int len)
        {
            byte[] b = new byte[len];
            for (int i = 0; i < len; i++) b[i] = (byte)text[i];
            return WriteAllBytes(path, b, len);
        }

        /// <summary>True if the file exists and is readable.</summary>
        public static unsafe bool Exists(string path)
        {
            byte* p = stackalloc byte[256];
            PathBytes(path, p, 256);
            byte* one = stackalloc byte[1];
            return bz_fs_read(p, one, 1) > 0;
        }
    }

    /// <summary>One entry from a directory listing.</summary>
    public sealed class BzFileInfo
    {
        public char[] Name;
        public int NameLen;
        public bool IsDirectory;
        public BzFileInfo Next;
    }

    /// <summary>Directory enumeration (System.IO.Directory) over FS_LIST. An empty
    /// path lists the mount points; a mount path lists its files.</summary>
    public static class BzDir
    {
        [DllImport("*")] static extern unsafe ulong bz_fs_list(byte* path, byte* buf, ulong max);

        const int ENTRY_SIZE = 32;   // abi::FsEntry
        const int NAME_MAX = 24;
        const int ISDIR_OFF = 24;

        /// <summary>List a directory; returns the head of a linked list (null if empty).</summary>
        public static unsafe BzFileInfo GetEntries(char[] path, int plen, int max)
        {
            if (max <= 0) max = 64;
            byte* p = stackalloc byte[256];
            BzFile.PathBytes(path, plen, p, 256);
            byte[] buf = new byte[ENTRY_SIZE * max];
            ulong n;
            fixed (byte* b = buf) n = bz_fs_list(p, b, (ulong)max);
            BzFileInfo head = null, tail = null;
            for (ulong i = 0; i < n; i++)
            {
                int b = (int)i * ENTRY_SIZE;
                BzFileInfo e = new BzFileInfo();
                char[] nm = new char[NAME_MAX];
                int nl = 0;
                for (int k = 0; k < NAME_MAX; k++)
                {
                    byte c = buf[b + k];
                    if (c == 0) break;
                    nm[nl++] = (char)c;
                }
                e.Name = nm; e.NameLen = nl;
                e.IsDirectory = BzProcess.ReadU64(buf, b + ISDIR_OFF) != 0;
                if (head == null) { head = e; tail = e; } else { tail.Next = e; tail = e; }
            }
            return head;
        }

        public static BzFileInfo GetEntries(string path, int max)
        {
            char[] t = new char[path.Length];
            for (int i = 0; i < path.Length; i++) t[i] = path[i];
            return GetEntries(t, path.Length, max);
        }

        /// <summary>The mount points (an empty path to FS_LIST).</summary>
        public static BzFileInfo GetMounts() => GetEntries("", 32);

        public static int Count(BzFileInfo head)
        {
            int n = 0;
            BzFileInfo e = head;
            while (e != null) { n++; e = e.Next; }
            return n;
        }

        /// <summary>True if a listing contains `name` (case-sensitive).</summary>
        public static bool Contains(BzFileInfo head, string name)
        {
            BzFileInfo e = head;
            while (e != null)
            {
                if (e.NameLen == name.Length)
                {
                    bool same = true;
                    for (int i = 0; i < e.NameLen; i++) if (e.Name[i] != name[i]) { same = false; break; }
                    if (same) return true;
                }
                e = e.Next;
            }
            return false;
        }
    }

    /// <summary>An in-memory stream over a byte[] (System.IO.MemoryStream), with
    /// the read/write/seek surface the rest of the BCL needs.</summary>
    public sealed class BzMemoryStream
    {
        byte[] _buf;
        int _len;
        int _pos;

        public BzMemoryStream() { _buf = new byte[64]; _len = 0; _pos = 0; }
        public BzMemoryStream(byte[] data, int len) { _buf = data; _len = len; _pos = 0; }

        public int Length => _len;
        public int Position { get { return _pos; } set { _pos = value < 0 ? 0 : (value > _len ? _len : value); } }
        public byte[] Buffer => _buf;

        void Ensure(int extra)
        {
            if (_pos + extra <= _buf.Length) return;
            int cap = _buf.Length * 2;
            while (cap < _pos + extra) cap *= 2;
            byte[] b = new byte[cap];
            for (int i = 0; i < _len; i++) b[i] = _buf[i];
            _buf = b;
        }

        /// <summary>Read up to `count` bytes into `dst`; returns the count read.</summary>
        public int Read(byte[] dst, int offset, int count)
        {
            int n = _len - _pos;
            if (n > count) n = count;
            if (n <= 0) return 0;
            for (int i = 0; i < n; i++) dst[offset + i] = _buf[_pos + i];
            _pos += n;
            return n;
        }

        public int ReadByte() => _pos < _len ? _buf[_pos++] : -1;

        public void Write(byte[] src, int offset, int count)
        {
            Ensure(count);
            for (int i = 0; i < count; i++) _buf[_pos + i] = src[offset + i];
            _pos += count;
            if (_pos > _len) _len = _pos;
        }

        public void WriteByte(byte b)
        {
            Ensure(1);
            _buf[_pos++] = b;
            if (_pos > _len) _len = _pos;
        }

        public void Seek(int offset) => Position = offset;
        public void SetLength(int len) { _len = len; if (_pos > _len) _pos = _len; }

        /// <summary>Copy the contents into a right-sized array.</summary>
        public byte[] ToArray()
        {
            byte[] r = new byte[_len];
            for (int i = 0; i < _len; i++) r[i] = _buf[i];
            return r;
        }
    }

    // =====================================================================
    // System.Net / System.Net.Sockets / System.Net.Http
    // =====================================================================

    /// <summary>An IPv4 address (System.Net.IPAddress). Four octets, no DNS —
    /// the kernel stack has no resolver.</summary>
    public sealed class BzIPAddress
    {
        public byte A, B, C, D;

        public BzIPAddress(byte a, byte b, byte c, byte d) { A = a; B = b; C = c; D = d; }

        /// <summary>The loopback address the kernel stack is configured with.</summary>
        public static BzIPAddress Loopback() => new BzIPAddress(127, 0, 0, 1);

        /// <summary>Parse dotted-quad text ("10.0.2.15"); null if malformed.</summary>
        public static BzIPAddress Parse(char[] s, int len)
        {
            int[] o = new int[4];
            int part = 0, v = 0, digits = 0;
            for (int i = 0; i < len; i++)
            {
                char c = s[i];
                if (c >= '0' && c <= '9') { v = v * 10 + (c - '0'); digits++; if (v > 255) return null; }
                else if (c == '.') { if (digits == 0 || part >= 3) return null; o[part++] = v; v = 0; digits = 0; }
                else return null;
            }
            if (part != 3 || digits == 0) return null;
            o[3] = v;
            return new BzIPAddress((byte)o[0], (byte)o[1], (byte)o[2], (byte)o[3]);
        }

        public static BzIPAddress Parse(string s)
        {
            char[] t = new char[s.Length];
            for (int i = 0; i < s.Length; i++) t[i] = s[i];
            return Parse(t, s.Length);
        }

        /// <summary>Write dotted-quad text into `dst`; returns the length.</summary>
        public int Format(char[] dst)
        {
            int n = BzCulture.FormatIntAt(A, dst, 0, false, ',');
            if (n < dst.Length) dst[n++] = '.';
            n = BzCulture.FormatIntAt(B, dst, n, false, ',');
            if (n < dst.Length) dst[n++] = '.';
            n = BzCulture.FormatIntAt(C, dst, n, false, ',');
            if (n < dst.Length) dst[n++] = '.';
            return BzCulture.FormatIntAt(D, dst, n, false, ',');
        }

        public bool Equals(BzIPAddress o) => o != null && A == o.A && B == o.B && C == o.C && D == o.D;
    }

    /// <summary>Interface state and counters (System.Net.NetworkInformation).</summary>
    public sealed class BzNetInfo
    {
        [DllImport("*")] static extern unsafe ulong bz_net_info(byte* outp);

        public BzIPAddress Address;
        public bool Up;
        public ulong SentDatagrams, ReceivedDatagrams, IcmpReplies, ArpReplies;

        public static unsafe BzNetInfo Query()
        {
            byte[] buf = new byte[48];
            fixed (byte* p = buf) bz_net_info(p);
            BzNetInfo i = new BzNetInfo();
            i.Address = new BzIPAddress(buf[0], buf[1], buf[2], buf[3]);
            i.Up = BzProcess.ReadU64(buf, 8) != 0;
            i.SentDatagrams = BzProcess.ReadU64(buf, 16);
            i.ReceivedDatagrams = BzProcess.ReadU64(buf, 24);
            i.IcmpReplies = BzProcess.ReadU64(buf, 32);
            i.ArpReplies = BzProcess.ReadU64(buf, 40);
            return i;
        }
    }

    /// <summary>A UDP datagram socket (System.Net.Sockets.Socket / UdpClient) over
    /// the NET_* syscalls. The kernel stack is Ethernet + ARP + IPv4 + ICMP + UDP
    /// on a loopback device, so traffic reaches this machine only — there is no
    /// NIC driver yet. Receives are non-blocking: <see cref="Receive"/> returns 0
    /// when nothing is queued.
    ///
    /// TCP is not implemented (<see cref="BzSocketKind.Stream"/> fails), which is
    /// why <see cref="BzHttp"/> is a message builder/parser rather than a client.</summary>
    public sealed class BzSocket
    {
        [DllImport("*")] static extern ulong bz_net_socket(ulong kind);
        [DllImport("*")] static extern ulong bz_net_bind(ulong handle, ulong port);
        [DllImport("*")] static extern unsafe ulong bz_net_send(ulong handle, byte* buf, ulong len);
        [DllImport("*")] static extern unsafe ulong bz_net_recv(ulong handle, byte* buf, ulong max);
        [DllImport("*")] static extern ulong bz_net_close(ulong handle);

        /// <summary>Header bytes before the payload in the syscall buffer.</summary>
        public const int HeaderSize = 16;
        /// <summary>Largest payload the kernel will accept.</summary>
        public const int MaxPayload = 1024;

        ulong _handle;
        int _localPort;

        /// <summary>Peer address and port of the datagram most recently received.</summary>
        public BzIPAddress RemoteAddress;
        public int RemotePort;

        BzSocket(ulong handle) { _handle = handle; }

        public bool IsOpen => _handle != 0;
        public int LocalPort => _localPort;

        /// <summary>Open a UDP socket; null if the stack is down.</summary>
        public static BzSocket CreateUdp()
        {
            ulong h = bz_net_socket(0);
            return h == 0 ? null : new BzSocket(h);
        }

        /// <summary>Bind to a local port so datagrams sent there are delivered here.</summary>
        public bool Bind(int port)
        {
            if (_handle == 0 || port <= 0 || port > 65535) return false;
            if (bz_net_bind(_handle, (ulong)port) != 0) return false;
            _localPort = port;
            return true;
        }

        /// <summary>Send `len` bytes to `dest:port`. Returns bytes sent.</summary>
        public unsafe int SendTo(BzIPAddress dest, int port, byte[] data, int len)
        {
            if (_handle == 0 || dest == null || len < 0 || len > MaxPayload) return 0;
            byte[] buf = new byte[HeaderSize + len];
            buf[0] = dest.A; buf[1] = dest.B; buf[2] = dest.C; buf[3] = dest.D;
            buf[4] = (byte)(port & 0xFF);
            buf[5] = (byte)((port >> 8) & 0xFF);
            buf[6] = 0; buf[7] = 0;
            buf[8] = (byte)(len & 0xFF);
            buf[9] = (byte)((len >> 8) & 0xFF);
            for (int i = 10; i < 16; i++) buf[i] = 0;
            for (int i = 0; i < len; i++) buf[HeaderSize + i] = data[i];
            fixed (byte* p = buf) return (int)bz_net_send(_handle, p, (ulong)len);
        }

        /// <summary>Send ASCII text. Returns bytes sent.</summary>
        public int SendTo(BzIPAddress dest, int port, string text)
        {
            byte[] b = new byte[text.Length];
            for (int i = 0; i < text.Length; i++) b[i] = (byte)text[i];
            return SendTo(dest, port, b, text.Length);
        }

        /// <summary>Try to receive one datagram into `data`. Returns the payload
        /// length, or 0 when none is queued. Sets RemoteAddress/RemotePort.</summary>
        public unsafe int Receive(byte[] data, int max)
        {
            if (_handle == 0) return 0;
            if (max > MaxPayload) max = MaxPayload;
            byte[] buf = new byte[HeaderSize + max];
            ulong n;
            fixed (byte* p = buf) n = bz_net_recv(_handle, p, (ulong)max);
            if (n == 0) return 0;
            RemoteAddress = new BzIPAddress(buf[0], buf[1], buf[2], buf[3]);
            RemotePort = buf[4] | (buf[5] << 8);
            int len = (int)n;
            if (len > max) len = max;
            for (int i = 0; i < len && i < data.Length; i++) data[i] = buf[HeaderSize + i];
            return len;
        }

        /// <summary>Poll for a datagram up to `attempts` times (the loopback
        /// delivers synchronously, so one attempt is normally enough).</summary>
        public int ReceiveWithRetry(byte[] data, int max, int attempts)
        {
            for (int i = 0; i < attempts; i++)
            {
                int n = Receive(data, max);
                if (n > 0) return n;
            }
            return 0;
        }

        public void Close()
        {
            if (_handle != 0) { bz_net_close(_handle); _handle = 0; }
        }
    }

    /// <summary>Socket kinds. Only <see cref="Dgram"/> works today.</summary>
    public static class BzSocketKind
    {
        public const int Dgram = 0;
        /// <summary>Reserved for TCP; NET_SOCKET rejects it.</summary>
        public const int Stream = 1;
    }

    /// <summary>HTTP message building and parsing (System.Net.Http). This is the
    /// protocol layer only: it produces a well-formed request and understands a
    /// response, but it cannot open a connection because the kernel has no TCP
    /// yet. Once TCP lands, a client is these two methods plus a stream.</summary>
    public static class BzHttp
    {
        /// <summary>Build "GET path HTTP/1.1" with a Host header into `dst`.
        /// Returns the length written.</summary>
        public static int BuildGet(string host, string path, char[] dst)
        {
            int n = 0;
            n = Put(dst, n, "GET ");
            n = Put(dst, n, path);
            n = Put(dst, n, " HTTP/1.1\r\nHost: ");
            n = Put(dst, n, host);
            n = Put(dst, n, "\r\nConnection: close\r\nUser-Agent: Buitenzorg/1.0\r\n\r\n");
            return n;
        }

        /// <summary>Build a POST with an ASCII body and a Content-Length header.</summary>
        public static int BuildPost(string host, string path, char[] body, int bodyLen, char[] dst)
        {
            int n = 0;
            n = Put(dst, n, "POST ");
            n = Put(dst, n, path);
            n = Put(dst, n, " HTTP/1.1\r\nHost: ");
            n = Put(dst, n, host);
            n = Put(dst, n, "\r\nContent-Type: text/plain\r\nContent-Length: ");
            n = BzCulture.FormatIntAt(bodyLen, dst, n, false, ',');
            n = Put(dst, n, "\r\nConnection: close\r\n\r\n");
            for (int i = 0; i < bodyLen && n < dst.Length; i++) dst[n++] = body[i];
            return n;
        }

        static int Put(char[] dst, int n, string s)
        {
            for (int i = 0; i < s.Length && n < dst.Length; i++) dst[n++] = s[i];
            return n;
        }

        /// <summary>Parse a response's status line. Returns the status code, or
        /// -1 if the input is not an HTTP response. `bodyStart` receives the
        /// index just past the blank line separating headers from the body.</summary>
        public static int ParseStatus(char[] resp, int len, out int bodyStart)
        {
            bodyStart = -1;
            if (len < 12) return -1;
            if (resp[0] != 'H' || resp[1] != 'T' || resp[2] != 'T' || resp[3] != 'P') return -1;
            int sp = -1;
            for (int i = 0; i < len; i++) if (resp[i] == ' ') { sp = i; break; }
            if (sp < 0 || sp + 3 >= len) return -1;
            int code = 0;
            for (int i = sp + 1; i < sp + 4; i++)
            {
                if (resp[i] < '0' || resp[i] > '9') return -1;
                code = code * 10 + (resp[i] - '0');
            }
            for (int i = 0; i + 3 < len; i++)
                if (resp[i] == '\r' && resp[i + 1] == '\n' && resp[i + 2] == '\r' && resp[i + 3] == '\n')
                { bodyStart = i + 4; break; }
            return code;
        }

        /// <summary>Find a header's value. Returns its length (0 if absent) and
        /// copies it into `value`.</summary>
        public static int GetHeader(char[] resp, int len, string name, char[] value)
        {
            int i = 0;
            while (i < len)
            {
                // start of a header line
                int lineEnd = i;
                while (lineEnd + 1 < len && !(resp[lineEnd] == '\r' && resp[lineEnd + 1] == '\n')) lineEnd++;
                if (lineEnd == i) break;   // blank line: headers are done
                bool match = i + name.Length < lineEnd;
                if (match)
                    for (int k = 0; k < name.Length; k++)
                    {
                        char a = resp[i + k], b = name[k];
                        if (a >= 'A' && a <= 'Z') a = (char)(a + 32);
                        if (b >= 'A' && b <= 'Z') b = (char)(b + 32);
                        if (a != b) { match = false; break; }
                    }
                if (match && resp[i + name.Length] == ':')
                {
                    int v = i + name.Length + 1;
                    while (v < lineEnd && resp[v] == ' ') v++;
                    int n = 0;
                    for (int k = v; k < lineEnd && n < value.Length; k++) value[n++] = resp[k];
                    return n;
                }
                i = lineEnd + 2;
            }
            return 0;
        }
    }

    // =====================================================================
    // System.Threading.Tasks
    // =====================================================================

    /// <summary>A cooperative task (System.Threading.Tasks.Task) over the ring-3
    /// thread syscalls. The body is a **function pointer**, not a delegate: a
    /// method-group conversion caches the delegate in a GC static, which zerolib
    /// does not initialize.
    ///
    /// Threads here are cooperative — a task runs until it yields or finishes —
    /// and there is no thread pool, so every <see cref="Run"/> creates a thread.</summary>
    public sealed class BzTask
    {
        [DllImport("*")] static extern ulong bz_thread_create(ulong entry, ulong arg);
        [DllImport("*")] static extern ulong bz_thread_join(ulong tid);
        [DllImport("*")] static extern void bz_yield();

        ulong _tid;
        bool _completed;

        BzTask(ulong tid) { _tid = tid; }

        public ulong Id => _tid;
        public bool IsCompleted => _completed;

        /// <summary>Start `body(arg)` on a new cooperative thread.
        /// Returns null if the thread could not be created.</summary>
        public static unsafe BzTask Run(delegate*<ulong, void> body, ulong arg)
        {
            ulong tid = bz_thread_create((ulong)body, arg);
            return tid == 0 ? null : new BzTask(tid);
        }

        /// <summary>Block (cooperatively) until this task finishes.</summary>
        public bool Wait()
        {
            if (_completed) return true;
            bool ok = bz_thread_join(_tid) == 0;
            _completed = true;
            return ok;
        }

        /// <summary>Wait for every task in a list.</summary>
        public static bool WhenAll(BzRefList<BzTask> tasks)
        {
            bool ok = true;
            for (int i = 0; i < tasks.Count; i++)
            {
                BzTask t = tasks.Get(i);
                if (t != null && !t.Wait()) ok = false;
            }
            return ok;
        }

        /// <summary>Give other runnable tasks a turn.</summary>
        public static void Yield() => bz_yield();
    }

    /// <summary>A futex-backed mutex (System.Threading.Mutex) for guarding state
    /// shared between tasks. Uncontended locks never enter the kernel; a
    /// contended one blocks on FUTEX_WAIT rather than spinning.</summary>
    public sealed unsafe class BzMutex
    {
        [DllImport("*")] static extern unsafe void bz_mutex_lock(int* m);
        [DllImport("*")] static extern unsafe void bz_mutex_unlock(int* m);

        int* _state;

        /// <summary>Wrap a caller-owned lock word (it must outlive the mutex and
        /// be shared by every task that locks it).</summary>
        public BzMutex(int* state) { _state = state; *_state = 0; }

        public void Lock() => bz_mutex_lock(_state);
        public void Unlock() => bz_mutex_unlock(_state);
    }

    // =====================================================================
    // System.Timers
    // =====================================================================

    /// <summary>A polled periodic timer (System.Timers.Timer). Ring-3 apps have no
    /// signal delivery, so the app pumps it from its own loop: call
    /// <see cref="Poll"/> and act when it returns true. Intervals are in kernel
    /// timer ticks (see <see cref="BzSystemInfo.TickHz"/>, ~18.2 Hz).</summary>
    public sealed class BzTimer
    {
        [DllImport("*")] static extern ulong bz_ticks();

        ulong _interval;
        ulong _next;
        bool _enabled;
        bool _autoReset;
        ulong _count;

        public BzTimer(ulong intervalTicks)
        {
            _interval = intervalTicks == 0 ? 1 : intervalTicks;
            _autoReset = true;
            _next = bz_ticks() + _interval;
        }

        public bool AutoReset { get { return _autoReset; } set { _autoReset = value; } }
        public bool Enabled => _enabled;
        public ulong Count => _count;
        public ulong Interval { get { return _interval; } set { _interval = value == 0 ? 1 : value; } }

        public void Start() { _enabled = true; _next = bz_ticks() + _interval; }
        public void Stop() => _enabled = false;

        /// <summary>True once per elapsed interval. Missed intervals do not pile
        /// up: the deadline is advanced past `now`.</summary>
        public bool Poll()
        {
            if (!_enabled) return false;
            ulong now = bz_ticks();
            if (now < _next) return false;
            _count++;
            if (_autoReset)
            {
                _next += _interval;
                if (_next <= now) _next = now + _interval;   // we fell behind; resync
            }
            else _enabled = false;
            return true;
        }

        /// <summary>Ticks remaining until the next fire (0 if it is due).</summary>
        public ulong Remaining()
        {
            ulong now = bz_ticks();
            return _next > now ? _next - now : 0;
        }
    }
}

// Buitenzorg.Diagnostics.Profiler -- named timing zones for app authors.
//
//   using (Profiler.Zone("render")) { ... }
//
// The time of each scope is added to this program's zone in the kernel
// (syscall PROF_ZONE); shell `prof zones` (or `prof zones PID`) lists calls,
// total and longest per zone, and the zones are printed to the log when the
// program exits. Off the device the totals are kept in `Totals`.

using System.Collections.Generic;
using System.Diagnostics;
using Buitenzorg.App;

namespace Buitenzorg.Diagnostics;

public static class Profiler
{
    static bool _offDevice;

    /// <summary>Off the device: zone name -> (calls, total ns).</summary>
    public static readonly Dictionary<string, long[]> Totals = new();

    /// <summary>Time the scope of a `using` block as zone <paramref name="name"/>.</summary>
    public static Scope Zone(string name) => new Scope(name);

    /// <summary>Add <paramref name="ns"/> nanoseconds to zone <paramref name="name"/>
    /// (at most 31 characters). False when refused (empty name, too many zones).</summary>
    public static bool Add(string name, long ns)
    {
        if (string.IsNullOrEmpty(name) || ns < 0) return false;
        if (!_offDevice)
        {
            try { return Native.ProfZone(name, ns) == 0; }
            catch (System.Exception e) when (e is System.DllNotFoundException || e is System.EntryPointNotFoundException) { _offDevice = true; }
        }
        if (!Totals.TryGetValue(name, out long[]? t))
        {
            if (Totals.Count >= 64) return false;
            t = new long[2];
            Totals[name] = t;
        }
        t[0]++;
        t[1] += ns;
        return true;
    }

    public sealed class Scope : System.IDisposable
    {
        readonly string _name;
        readonly Stopwatch _watch;
        bool _done;

        internal Scope(string name)
        {
            _name = name;
            _watch = Stopwatch.StartNew();
        }

        /// <summary>Nanoseconds so far.</summary>
        public long ElapsedNs => _watch.ElapsedTicks * 1_000_000_000L / Stopwatch.Frequency;

        public void Dispose()
        {
            if (_done) return;
            _done = true;
            Add(_name, ElapsedNs);
        }
    }
}

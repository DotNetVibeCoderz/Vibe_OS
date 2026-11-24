// Buitenzorg.Net.NetworkConfig -- the machine's network for settings apps:
// what link is up (cable / Wi-Fi / none), its addresses, the Wi-Fi adapter,
// scanning and joining networks, asking DHCP again.
//
// On the device it reads the shell's `netinfo` (key=value lines) and runs
// `wifi scan --raw`, `wifi connect`, `wifi forget` and `netup` through
// SHELL_RUN. Off the device a simulated machine answers (a cable at
// 10.0.2.15, a Wi-Fi adapter that sees three networks; "KebunRaya" joins
// with the password "rahasia123"), so `dotnet X.dll --selftest` works.

using Buitenzorg.App;

namespace Buitenzorg.Net;

public enum LinkKind { None, Wired, Wireless }

/// <summary>The network state at one moment.</summary>
public sealed class NetworkStatus
{
    public LinkKind Link;
    public string Mac = "", Address = "", Gateway = "", Dns = "";
    public int Prefix;
    public long FramesSent, FramesReceived;
    public List<string> Ipv6 = new();
    /// <summary>What bringing up the network came to (kernel's last word).</summary>
    public string Message = "";
    /// <summary>The Wi-Fi adapter ("" = none) and whether its driver can scan + join.</summary>
    public string WifiAdapter = "";
    public bool WifiCanScan;
    /// <summary>The network joined at every start ("" = none).</summary>
    public string SavedNetwork = "";

    public bool Connected => Link != LinkKind.None && Address.Length > 0;
    public string LinkText => Link switch
    {
        LinkKind.Wired => "Kabel (Ethernet)",
        LinkKind.Wireless => "Wi-Fi",
        _ => "Tidak tersambung",
    };
}

/// <summary>A network a scan found.</summary>
public sealed class WifiNetwork
{
    public string Ssid = "";
    public int Rssi, Channel;
    public bool Secured;
    /// <summary>0..4 bars from the signal strength.</summary>
    public int Bars => Rssi >= -55 ? 4 : Rssi >= -65 ? 3 : Rssi >= -75 ? 2 : Rssi >= -85 ? 1 : 0;
    public override string ToString() => (Ssid.Length == 0 ? "(tersembunyi)" : Ssid) + (Secured ? "  [aman]" : "  [terbuka]") + "  " + Rssi + " dBm";
}

public static class NetworkConfig
{
    // --- the simulated machine (off the device) ---
    static bool _simWifi;
    static string _simSaved = "";

    static bool OnDevice => Shell.IsAvailable;

    /// <summary>Read the current state.</summary>
    public static NetworkStatus Status()
    {
        if (!OnDevice) return SimStatus();
        return Parse(Shell.Run("netinfo").Lines);
    }

    /// <summary>Parse `netinfo` output (public for tests).</summary>
    public static NetworkStatus Parse(IEnumerable<string> lines)
    {
        var s = new NetworkStatus();
        foreach (var raw in lines)
        {
            string l = raw.TrimEnd('\r');
            int eq = l.IndexOf('=');
            if (eq <= 0) continue;
            string k = l.Substring(0, eq), v = l.Substring(eq + 1).Trim();
            switch (k)
            {
                case "link": s.Link = v == "wired" ? LinkKind.Wired : v == "wifi" ? LinkKind.Wireless : LinkKind.None; break;
                case "mac": s.Mac = v; break;
                case "ip": s.Address = v; break;
                case "prefix": int.TryParse(v, out s.Prefix); break;
                case "gateway": s.Gateway = v; break;
                case "dns": s.Dns = v; break;
                case "tx": long.TryParse(v, out s.FramesSent); break;
                case "rx": long.TryParse(v, out s.FramesReceived); break;
                case "ipv6": if (v.Length > 0) s.Ipv6.Add(v); break;
                case "status": s.Message = v; break;
                case "wifi": s.WifiAdapter = v; break;
                case "wifi_scan": s.WifiCanScan = v == "1"; break;
                case "wifi_saved": s.SavedNetwork = v; break;
            }
        }
        if (s.Link == LinkKind.None) s.Address = "";
        return s;
    }

    /// <summary>Scan for networks (takes a few seconds). Null + `error` when it cannot.</summary>
    public static List<WifiNetwork>? Scan(out string error)
    {
        error = "";
        if (!OnDevice)
        {
            return new List<WifiNetwork>
            {
                new WifiNetwork { Ssid = "KebunRaya", Rssi = -48, Channel = 6, Secured = true },
                new WifiNetwork { Ssid = "Tamu-Bogor", Rssi = -67, Channel = 11, Secured = false },
                new WifiNetwork { Ssid = "Anggrek5G", Rssi = -79, Channel = 36, Secured = true },
            };
        }
        var r = Shell.Run("wifi scan --raw");
        var list = new List<WifiNetwork>();
        var other = new List<string>();
        foreach (var raw in r.Lines)
        {
            string l = raw.TrimEnd('\r');
            if (!l.StartsWith("@bss\t")) { if (l.Trim().Length > 0) other.Add(l.Trim()); continue; }
            var f = l.Split('\t');
            if (f.Length < 5) continue;
            var n = new WifiNetwork { Secured = f[3] == "wpa", Ssid = f[4] };
            int.TryParse(f[1], out n.Rssi);
            int.TryParse(f[2], out n.Channel);
            list.Add(n);
        }
        if (list.Count == 0 && other.Count > 0) { error = string.Join(" ", other); return null; }
        // strongest first, one entry per name
        list.Sort((a, b) => b.Rssi.CompareTo(a.Rssi));
        var seen = new List<string>();
        var unique = new List<WifiNetwork>();
        foreach (var n in list)
        {
            if (n.Ssid.Length > 0 && seen.Contains(n.Ssid)) continue;
            seen.Add(n.Ssid);
            unique.Add(n);
        }
        return unique;
    }

    /// <summary>Join `ssid` (empty password = an open network). Null = joined (and saved
    /// for every start), else why not.</summary>
    public static string? Connect(string ssid, string password)
    {
        if (ssid.Length == 0) return "pilih jaringan dulu";
        if (ssid.IndexOf('\'') >= 0 || password.IndexOf('\'') >= 0) return "nama atau kata sandi dengan tanda kutip tunggal (') belum didukung";
        if (password.Length > 0 && (password.Length < 8 || password.Length > 63)) return "kata sandi WPA2 harus 8 sampai 63 karakter";
        if (!OnDevice)
        {
            if (ssid == "Tamu-Bogor" || (ssid == "KebunRaya" && password == "rahasia123")) { _simWifi = true; _simSaved = ssid; return null; }
            return ssid == "KebunRaya" ? "kata sandi salah (jaringan menolak kunci)" : "jaringan tidak menjawab";
        }
        var r = Shell.Run("wifi connect '" + ssid + "'" + (password.Length > 0 ? " '" + password + "'" : ""));
        string text = r.Output.Trim();
        return text.Contains("connected to") ? null : (text.StartsWith("wifi: ") ? text.Substring(6) : text);
    }

    /// <summary>Forget the saved network (it is no longer joined at start).</summary>
    public static void Forget()
    {
        if (!OnDevice) { _simSaved = ""; return; }
        Shell.Run("wifi forget");
    }

    /// <summary>Ask the platform to bring the network up again (DHCP). False = it cannot.</summary>
    public static bool Renew()
    {
        if (!OnDevice) return true;
        return !Shell.Run("netup").Output.Contains("tidak ada");
    }

    static NetworkStatus SimStatus()
    {
        var s = new NetworkStatus
        {
            Link = _simWifi ? LinkKind.Wireless : LinkKind.Wired,
            Mac = "52:54:00:12:34:56",
            Address = _simWifi ? "192.168.1.23" : "10.0.2.15",
            Prefix = 24,
            Gateway = _simWifi ? "192.168.1.1" : "10.0.2.2",
            Dns = _simWifi ? "192.168.1.1" : "10.0.2.3",
            FramesSent = 120,
            FramesReceived = 140,
            Message = "(simulasi)",
            WifiAdapter = "Wi-Fi simulasi",
            WifiCanScan = true,
            SavedNetwork = _simSaved,
        };
        s.Ipv6.Add("fe80::5054:ff:fe12:3456/64");
        return s;
    }
}

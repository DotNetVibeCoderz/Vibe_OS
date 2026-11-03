// Buitenzorg.App -- user accounts (USER_OP syscall): list, verify, add, remove,
// change / reset a password, profile picture. Passwords are checked and
// stored (salted PBKDF2-HMAC-SHA256) by the kernel; an app never sees a hash.
// Off the device an in-memory simulation answers, with the same default
// account (administrator / ganteng@ku).

using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace Buitenzorg.App;

internal static class UsersNative
{
    [DllImport("buitenzorg")] internal static extern int UserOp(int op, string request);
    [DllImport("buitenzorg")] internal static extern string UserList();
}

/// <summary>One account.</summary>
public sealed class UserAccount
{
    public string Name { get; }
    public bool IsAdministrator { get; }
    /// <summary>Path of the profile picture (a BMP, e.g. /disk/AVATAR1.BMP).</summary>
    public string Avatar { get; }
    public UserAccount(string name, bool admin, string avatar) { Name = name; IsAdministrator = admin; Avatar = avatar; }
    public override string ToString() => Name;
}

/// <summary>The system's user accounts.</summary>
public static class Users
{
    public const string DefaultAdministrator = "administrator";

    /// <summary>The built-in profile pictures.</summary>
    public static readonly string[] Avatars =
        { "/disk/AVATAR1.BMP", "/disk/AVATAR2.BMP", "/disk/AVATAR3.BMP", "/disk/AVATAR4.BMP", "/disk/AVATAR5.BMP", "/disk/AVATAR6.BMP" };

    sealed class SimUser { public string Password = ""; public bool Admin; public string Avatar = ""; }
    static Dictionary<string, SimUser>? _sim;
    static readonly List<string> _simOrder = new List<string>();

    static bool Fallback(Exception e)
    {
        if (e is not (DllNotFoundException or EntryPointNotFoundException)) return false;
        if (_sim == null)
        {
            _sim = new Dictionary<string, SimUser>();
            _sim[DefaultAdministrator] = new SimUser { Password = "ganteng@ku", Admin = true, Avatar = Avatars[0] };
            _simOrder.Add(DefaultAdministrator);
        }
        return true;
    }

    /// <summary>True off the device (the in-memory simulation answers).</summary>
    public static bool IsSimulated { get { _ = List(); return _sim != null; } }

    static int Op(int op, params string[] fields)
    {
        if (_sim != null) return SimOp(op, fields) ? 0 : -1;
        try { return UsersNative.UserOp(op, string.Join("\n", fields)); }
        catch (Exception e) when (Fallback(e)) { return Op(op, fields); }
    }

    public static List<UserAccount> List()
    {
        var list = new List<UserAccount>();
        if (_sim == null)
        {
            string text;
            try { text = UsersNative.UserList(); }
            catch (Exception e) when (Fallback(e)) { return List(); }
            foreach (var line in text.Split('\n'))
            {
                var f = line.Split('\t');
                if (f.Length >= 3 && f[0].Length > 0) list.Add(new UserAccount(f[0], f[1] == "1", f[2]));
            }
            return list;
        }
        foreach (var n in _simOrder) list.Add(new UserAccount(n, _sim[n].Admin, _sim[n].Avatar));
        return list;
    }

    public static bool Verify(string name, string password) => Op(1, name, password) == 0;
    public static bool Add(string admin, string adminPassword, string name, string password, bool administrator) =>
        Op(2, admin, adminPassword, name, password, administrator ? "1" : "0") == 0;
    public static bool Remove(string admin, string adminPassword, string name) => Op(3, admin, adminPassword, name) == 0;
    public static bool ChangePassword(string name, string current, string next) => Op(4, name, current, next) == 0;
    public static bool ResetPassword(string admin, string adminPassword, string name, string next) => Op(5, admin, adminPassword, name, next) == 0;
    /// <summary>Set the picture of `name`, with that user's password (or an administrator's as `who`).</summary>
    public static bool SetAvatar(string who, string password, string name, string path) => Op(6, who, password, name, path) == 0;

    /// <summary>The rules the kernel applies: 1-16 of a-z 0-9 _ - . starting with a letter.</summary>
    public static bool IsValidName(string n)
    {
        if (n.Length == 0 || n.Length > 16 || n[0] < 'a' || n[0] > 'z') return false;
        foreach (char c in n) if (!((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_' || c == '-' || c == '.')) return false;
        return true;
    }

    public static bool IsValidPassword(string p) => p.Length >= 4 && p.Length <= 64 && p.IndexOf('\n') < 0;

    static bool SimOp(int op, string[] f)
    {
        string A(int i) => i < f.Length ? f[i] : "";
        var s = _sim!;
        bool Admin(string n, string p) => s.TryGetValue(n, out var u) && u.Admin && u.Password == p;
        switch (op)
        {
            case 1: return s.TryGetValue(A(0), out var v) && v.Password == A(1);
            case 2:
                if (!Admin(A(0), A(1)) || !IsValidName(A(2)) || !IsValidPassword(A(3)) || s.ContainsKey(A(2))) return false;
                s[A(2)] = new SimUser { Password = A(3), Admin = A(4) == "1", Avatar = Avatars[s.Count % Avatars.Length] };
                _simOrder.Add(A(2));
                return true;
            case 3:
                if (!Admin(A(0), A(1)) || !s.TryGetValue(A(2), out var r)) return false;
                if (r.Admin) { int admins = 0; foreach (var x in s.Values) if (x.Admin) admins++; if (admins == 1) return false; }
                s.Remove(A(2)); _simOrder.Remove(A(2));
                return true;
            case 4:
                if (!s.TryGetValue(A(0), out var c) || c.Password != A(1) || !IsValidPassword(A(2))) return false;
                c.Password = A(2); return true;
            case 5:
                if (!Admin(A(0), A(1)) || !s.TryGetValue(A(2), out var t) || !IsValidPassword(A(3))) return false;
                t.Password = A(3); return true;
            case 6:
                if (!s.TryGetValue(A(2), out var w)) return false;
                if (!((A(0) == A(2) && w.Password == A(1)) || Admin(A(0), A(1)))) return false;
                w.Avatar = A(3); return true;
        }
        return false;
    }
}

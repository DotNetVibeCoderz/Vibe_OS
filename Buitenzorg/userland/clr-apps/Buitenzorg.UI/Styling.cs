// Buitenzorg.UI -- styles (Avalonia Styles): a selector such as
// "Button.accent:pointerover" or "ListBoxItem:selected, TabItem:selected"
// plus setters. Later styles win over earlier ones; local values win over
// every style. Pseudo-classes (:pointerover, :pressed, :focus, :disabled,
// :checked, :selected, :expanded, :open, :empty, ...) are set by the
// controls and restyle them as they change.

using System.Collections;

namespace Buitenzorg.UI;

/// <summary>One selector alternative: a type (or *), classes, pseudo-classes,
/// and optionally an ancestor it must sit inside ("TabControl Button").
/// As in Avalonia, "Button" matches that exact type only; ":is(Button)"
/// matches Button and every subclass.</summary>
public sealed class Selector
{
    readonly string? _type;
    readonly bool _derived;
    readonly string[] _classes;
    readonly Selector? _ancestor;

    Selector(string? type, bool derived, string[] classes, Selector? ancestor)
    {
        _type = type;
        _derived = derived;
        _classes = classes;
        _ancestor = ancestor;
    }

    /// <summary>Parse a selector list; ',' separates alternatives, ' ' a descendant.</summary>
    public static List<Selector> ParseList(string text)
    {
        var list = new List<Selector>();
        foreach (var alt in text.Split(','))
        {
            Selector? current = null;
            foreach (var part in alt.Trim().Split(' '))
            {
                if (part.Length == 0) continue;
                current = ParseOne(part, current);
            }
            if (current != null) list.Add(current);
        }
        return list;
    }

    static Selector ParseOne(string s, Selector? ancestor)
    {
        string? type = null;
        var classes = new List<string>();
        int i = 0;
        int start = 0;
        while (i < s.Length && s[i] != '.' && s[i] != ':') i++;
        if (i > 0) type = s.Substring(0, i);
        if (type == "*") type = null;
        bool derived = false;
        while (i < s.Length)
        {
            start = i;
            i++;
            while (i < s.Length && s[i] != '.' && s[i] != ':') i++;
            // pseudo-classes keep their colon (":pointerover"), classes drop the dot ("accent")
            string part = s.Substring(start, i - start);
            if (part.StartsWith(":is("))
            {
                // ":is(Button)" -- the parser stopped at no '.' or ':' inside the parentheses
                type = part.Substring(4, part.Length - 5);
                derived = true;
                continue;
            }
            classes.Add(part[0] == '.' ? part.Substring(1) : part);
        }
        return new Selector(type, derived, classes.ToArray(), ancestor);
    }

    /// <summary>Whether the type part can match (classes and pseudo-classes aside).</summary>
    public bool CouldMatchType(Control c) => _type == null || (_derived ? c.IsOfType(_type) : c.TypeName == _type);

    /// <summary>Classes, pseudo-classes and ancestor -- the type part is already known to match.</summary>
    internal bool MatchesGivenType(Control c)
    {
        foreach (var cls in _classes)
        {
            bool ok = cls[0] == ':' ? c.PseudoClasses.Contains(cls) : c.Classes.Contains(cls);
            if (!ok) return false;
        }
        if (_ancestor == null) return true;
        for (var p = c.Parent; p != null; p = p.Parent)
            if (_ancestor.Matches(p)) return true;
        return false;
    }

    public bool Matches(Control c)
    {
        if (!MatchesSelf(c)) return false;
        if (_ancestor == null) return true;
        for (var p = c.Parent; p != null; p = p.Parent)
            if (_ancestor.Matches(p)) return true;
        return false;
    }

    bool MatchesSelf(Control c)
    {
        if (!CouldMatchType(c)) return false;
        foreach (var cls in _classes)
        {
            bool ok = cls[0] == ':' ? c.PseudoClasses.Contains(cls) : c.Classes.Contains(cls);
            if (!ok) return false;
        }
        return true;
    }
}

/// <summary>A style plus those of its selector alternatives whose type matches one control type.</summary>
internal sealed class StyleCandidate
{
    public readonly Style Style;
    readonly Selector[] _sels;
    public StyleCandidate(Style style, Selector[] sels) { Style = style; _sels = sels; }

    public bool Matches(Control c)
    {
        foreach (var s in _sels) if (s.MatchesGivenType(c)) return true;
        return false;
    }
}

public sealed class Setter
{
    public UIProperty Property { get; }
    public object? Value { get; }
    public Setter(UIProperty property, object? value) { Property = property; Value = value; }
}

/// <summary>A style: selectors + setters (+ transitions). Collection-initialize the setters:
/// <c>new Style("Button.accent") { { Control.BackgroundProperty, 0x7CC4A4 } }</c>.</summary>
public sealed class Style : IEnumerable
{
    public List<Selector> Selectors { get; }
    public List<Setter> Setters { get; } = new List<Setter>();
    public string Text { get; }

    public Style(string selector)
    {
        Text = selector;
        Selectors = Selector.ParseList(selector);
    }

    public void Add(UIProperty property, object? value) => Setters.Add(new Setter(property, value));

    public bool Matches(Control c)
    {
        foreach (var s in Selectors)
            if (s.Matches(c)) return true;
        return false;
    }

    internal bool CouldMatchType(Control c)
    {
        foreach (var s in Selectors)
            if (s.CouldMatchType(c)) return true;
        return false;
    }

    IEnumerator IEnumerable.GetEnumerator() => Setters.GetEnumerator();
}

/// <summary>A style list; every change bumps <see cref="Version"/>, so controls restyle.</summary>
public sealed class Styles : List<Style>
{
    public static int Version { get; private set; }
    public new void Add(Style s) { base.Add(s); Version++; }
    public new void AddRange(IEnumerable<Style> s) { base.AddRange(s); Version++; }
    public new void Clear() { base.Clear(); Version++; }
    public new bool Remove(Style s) { Version++; return base.Remove(s); }
}

/// <summary>The classes of a control (Avalonia Classes): plain names, plus
/// pseudo-classes (":name") that only the control itself sets.</summary>
public sealed class Classes : IEnumerable
{
    readonly List<string> _items = new List<string>();
    readonly Control _owner;

    internal Classes(Control owner) { _owner = owner; }

    public int Count => _items.Count;

    public bool Contains(string name)
    {
        for (int i = 0; i < _items.Count; i++)
            if (_items[i] == name) return true;
        return false;
    }

    public void Add(string name)
    {
        if (Contains(name)) return;
        _items.Add(name);
        _owner.InvalidateStyle();
    }

    public void Remove(string name)
    {
        for (int i = 0; i < _items.Count; i++)
            if (_items[i] == name) { _items.RemoveAt(i); _owner.InvalidateStyle(); return; }
    }

    /// <summary>Add or remove a class.</summary>
    public void Set(string name, bool on)
    {
        if (on) Add(name); else Remove(name);
    }

    IEnumerator IEnumerable.GetEnumerator() => _items.GetEnumerator();
}

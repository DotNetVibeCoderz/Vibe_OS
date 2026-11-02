// Buitenzorg.UI -- the styled property system (Avalonia's StyledProperty /
// AvaloniaObject): every visual property has a value per priority -- a local
// value, an animated (transition) value, a style value -- and a default;
// inherited properties fall back to the parent. Styles, transitions and
// bindings all work through it.

namespace Buitenzorg.UI;

public abstract class UIProperty
{
    static readonly List<UIProperty> _all = new List<UIProperty>();
    /// <summary>Every registered property, by id.</summary>
    internal static List<UIProperty> All => _all;
    public int Id { get; }
    public string Name { get; }
    public bool Inherits { get; }
    public bool AffectsMeasure { get; }
    /// <summary>The default, boxed once (reads of unset properties must not allocate).</summary>
    internal object? BoxedDefault;

    protected UIProperty(string name, bool inherits, bool affectsMeasure)
    {
        Id = _all.Count;
        _all.Add(this);
        Name = name;
        Inherits = inherits;
        AffectsMeasure = affectsMeasure;
    }

    public override string ToString() => Name;
}

public sealed class StyledProperty<T> : UIProperty
{
    public readonly T Default;

    internal StyledProperty(string name, T def, bool inherits, bool affectsMeasure) : base(name, inherits, affectsMeasure)
    {
        Default = def;
        BoxedDefault = def;
    }
}

public static class UIProperties
{
    /// <summary>Register a styled (or attached) property.</summary>
    public static StyledProperty<T> Register<T>(string name, T defaultValue, bool inherits = false, bool affectsMeasure = false) =>
        new StyledProperty<T>(name, defaultValue, inherits, affectsMeasure);
}

/// <summary>An object with styled properties (Avalonia's AvaloniaObject).</summary>
public class UIObject
{
    // Per property id: one flags byte (which layers hold a value) and the
    // local, animated and style values. Reading an unset property touches
    // only the flags array and allocates nothing -- the interpreter reads
    // properties constantly during layout.
    const byte LocalFlag = 1, AnimFlag = 2, StyleFlag = 4;
    byte[] _flags = new byte[0];
    object?[] _local = new object?[0];
    object?[] _style = new object?[0];
    object?[] _anim = new object?[0];
    // Inherited values resolved from the ancestors, valid while InheritEpoch
    // is unchanged (any change of an inherited property, or of the tree shape,
    // bumps it).
    object?[] _inherited = new object?[0];
    int[] _inheritedEpoch = new int[0];
    internal static int InheritEpoch = 1;

    /// <summary>Raised after an effective value changed.</summary>
    public event Action<UIProperty>? PropertyChanged;

    internal virtual UIObject? InheritanceParent => null;

    void Ensure(int id)
    {
        if (id < _flags.Length) return;
        int n = Math.Max(id + 1, UIProperty.All.Count);
        Array.Resize(ref _flags, n);
        Array.Resize(ref _local, n);
        Array.Resize(ref _style, n);
        Array.Resize(ref _anim, n);
        Array.Resize(ref _inherited, n);
        Array.Resize(ref _inheritedEpoch, n);
    }

    internal object? GetBoxed(UIProperty p)
    {
        int id = p.Id;
        if (id < _flags.Length)
        {
            byte f = _flags[id];
            if (f != 0)
            {
                if ((f & LocalFlag) != 0) return _local[id];
                if ((f & AnimFlag) != 0) return _anim[id];
                return _style[id];
            }
        }
        if (p.Inherits)
        {
            var parent = InheritanceParent;
            if (parent != null) return parent.GetBoxed(p);
        }
        return p.BoxedDefault;
    }

    /// <summary>The value without the animated layer (a transition's target).</summary>
    internal object? GetBaseBoxed(UIProperty p)
    {
        int id = p.Id;
        if (id < _flags.Length)
        {
            byte f = _flags[id];
            if ((f & LocalFlag) != 0) return _local[id];
            if ((f & StyleFlag) != 0) return _style[id];
        }
        if (p.Inherits)
        {
            var parent = InheritanceParent;
            if (parent != null) return parent.GetBoxed(p);
        }
        return p.BoxedDefault;
    }

    public T GetValue<T>(StyledProperty<T> p)
    {
        int id = p.Id;
        if (id < _flags.Length)
        {
            byte f = _flags[id];
            if (f != 0)
            {
                object? v = (f & LocalFlag) != 0 ? _local[id] : (f & AnimFlag) != 0 ? _anim[id] : _style[id];
                return v == null ? default! : (T)v;
            }
        }
        if (p.Inherits)
        {
            var parent = InheritanceParent;
            if (parent != null)
            {
                Ensure(id);
                if (_inheritedEpoch[id] == InheritEpoch) { object? c = _inherited[id]; return c == null ? default! : (T)c; }
                T v = parent.GetValue(p);
                _inherited[id] = v;
                _inheritedEpoch[id] = InheritEpoch;
                return v;
            }
        }
        return p.Default;
    }

    public void SetValue<T>(StyledProperty<T> p, T value) => SetLocal(p, value);

    public bool IsSet(UIProperty p) => p.Id < _flags.Length && (_flags[p.Id] & LocalFlag) != 0;

    public void ClearValue(UIProperty p)
    {
        if (!IsSet(p)) return;
        object? old = GetBoxed(p);
        _flags[p.Id] &= unchecked((byte)~LocalFlag);
        _local[p.Id] = null;
        Changed(p, old);
    }

    internal void SetLocal(UIProperty p, object? value)
    {
        Ensure(p.Id);
        object? old = GetBoxed(p);
        _local[p.Id] = value;
        _flags[p.Id] |= LocalFlag;
        Changed(p, old);
    }

    internal void SetStyle(UIProperty p, object? value, bool has)
    {
        Ensure(p.Id);
        if (p.Inherits) InheritEpoch++;
        _style[p.Id] = value;
        if (has) _flags[p.Id] |= StyleFlag; else _flags[p.Id] &= unchecked((byte)~StyleFlag);
    }

    internal bool HasStyle(UIProperty p) => p.Id < _flags.Length && (_flags[p.Id] & StyleFlag) != 0;

    internal void SetAnimated(UIProperty p, object? value, bool has)
    {
        Ensure(p.Id);
        object? old = GetBoxed(p);
        _anim[p.Id] = value;
        if (has) _flags[p.Id] |= AnimFlag; else _flags[p.Id] &= unchecked((byte)~AnimFlag);
        Changed(p, old);
    }

    internal void Changed(UIProperty p, object? old)
    {
        if (p.Inherits) InheritEpoch++;
        object? now = GetBoxed(p);
        if (ReferenceEquals(old, now) || Equals(old, now)) return;
        OnPropertyChanged(p);
        PropertyChanged?.Invoke(p);
    }

    protected virtual void OnPropertyChanged(UIProperty p) { }
}

/// <summary>
/// Animates a property between style values (Avalonia Transitions): color
/// properties fade, int properties tween, both with a cubic ease-out.
/// </summary>
public sealed class Transition
{
    public UIProperty Property { get; }
    public int DurationMs { get; }

    public Transition(UIProperty property, int durationMs)
    {
        Property = property;
        DurationMs = durationMs;
    }
}

/// <summary>Running property animations, ticked by the event loop.</summary>
internal static class Animator
{
    sealed class Run
    {
        public UIObject Target = null!;
        public UIProperty Property = null!;
        public int From, To;
        public bool IsColor;
        public long Start;
        public int Duration;
    }

    static readonly List<Run> _runs = new List<Run>();

    public static bool Active => _runs.Count > 0;

    public static void Start(UIObject target, UIProperty p, int from, int to, int ms, bool color)
    {
        for (int i = _runs.Count - 1; i >= 0; i--)
            if (_runs[i].Target == target && _runs[i].Property == p) _runs.RemoveAt(i);
        if (from == to || ms <= 0 || (color && (from < 0 || to < 0))) { target.SetAnimated(p, null, false); return; }
        _runs.Add(new Run { Target = target, Property = p, From = from, To = to, IsColor = color, Start = Clock.Now, Duration = ms });
        target.SetAnimated(p, from, true);
    }

    public static void Tick()
    {
        long now = Clock.Now;
        for (int i = _runs.Count - 1; i >= 0; i--)
        {
            var r = _runs[i];
            long t = now - r.Start;
            if (t >= r.Duration)
            {
                _runs.RemoveAt(i);
                r.Target.SetAnimated(r.Property, null, false);
                continue;
            }
            // ease-out cubic: 1 - (1 - x)^3, in 1/256 steps
            int x = (int)(t * 256 / r.Duration);
            int inv = 256 - x;
            int e = 256 - inv * inv / 256 * inv / 256;
            int v = r.IsColor ? Color.Mix(r.From, r.To, e) : r.From + (r.To - r.From) * e / 256;
            r.Target.SetAnimated(r.Property, v, true);
        }
    }
}

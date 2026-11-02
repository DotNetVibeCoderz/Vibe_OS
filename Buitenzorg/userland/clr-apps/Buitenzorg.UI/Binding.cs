// Buitenzorg.UI -- MVVM: change notification, observable collections,
// commands and bindings. These live in Buitenzorg.UI (not System.*) because
// RustBCL does not implement System.ComponentModel / ObjectModel / Windows.Input.

using System.Collections;
using System.Runtime.CompilerServices;

namespace Buitenzorg.UI;

public delegate void PropertyChangedEventHandler(object sender, string propertyName);

public interface INotifyPropertyChanged
{
    event PropertyChangedEventHandler? PropertyChanged;
}

/// <summary>Base class for view models (CommunityToolkit-style SetProperty).</summary>
public abstract class ObservableObject : INotifyPropertyChanged
{
    public event PropertyChangedEventHandler? PropertyChanged;

    protected bool SetProperty<T>(ref T field, T value, [CallerMemberName] string name = "")
    {
        if (Equals(field, value)) return false;
        field = value;
        OnPropertyChanged(name);
        return true;
    }

    protected void OnPropertyChanged([CallerMemberName] string name = "") => PropertyChanged?.Invoke(this, name);
}

public enum CollectionChange { Add, Remove, Replace, Reset }

public delegate void CollectionChangedHandler(object sender, CollectionChange change, int index);

public interface INotifyCollectionChanged
{
    event CollectionChangedHandler? CollectionChanged;
}

/// <summary>A list that reports changes to the ItemsControls showing it.</summary>
public class ObservableCollection<T> : IEnumerable<T>, INotifyCollectionChanged
{
    readonly List<T> _items = new List<T>();

    public event CollectionChangedHandler? CollectionChanged;

    public ObservableCollection() { }
    public ObservableCollection(IEnumerable<T> items) { foreach (var i in items) _items.Add(i); }

    public int Count => _items.Count;

    public T this[int index]
    {
        get => _items[index];
        set { _items[index] = value; Raise(CollectionChange.Replace, index); }
    }

    public void Add(T item) { _items.Add(item); Raise(CollectionChange.Add, _items.Count - 1); }
    public void Insert(int index, T item) { _items.Insert(index, item); Raise(CollectionChange.Add, index); }
    public void RemoveAt(int index) { _items.RemoveAt(index); Raise(CollectionChange.Remove, index); }

    public bool Remove(T item)
    {
        int i = IndexOf(item);
        if (i < 0) return false;
        RemoveAt(i);
        return true;
    }

    public int IndexOf(T item)
    {
        for (int i = 0; i < _items.Count; i++)
            if (Equals(_items[i], item)) return i;
        return -1;
    }

    public bool Contains(T item) => IndexOf(item) >= 0;
    public void Clear() { _items.Clear(); Raise(CollectionChange.Reset, -1); }

    /// <summary>Replace every item with one notification.</summary>
    public void ResetWith(IEnumerable<T> items)
    {
        _items.Clear();
        foreach (var i in items) _items.Add(i);
        Raise(CollectionChange.Reset, -1);
    }

    void Raise(CollectionChange c, int index) => CollectionChanged?.Invoke(this, c, index);

    public IEnumerator<T> GetEnumerator() => _items.GetEnumerator();
    IEnumerator IEnumerable.GetEnumerator() => _items.GetEnumerator();
}

public interface ICommand
{
    bool CanExecute(object? parameter);
    void Execute(object? parameter);
    event Action? CanExecuteChanged;
}

/// <summary>A command from delegates (Avalonia ReactiveCommand / RelayCommand).</summary>
public sealed class RelayCommand : ICommand
{
    readonly Action<object?> _execute;
    readonly Func<object?, bool>? _can;

    public event Action? CanExecuteChanged;

    public RelayCommand(Action execute, Func<bool>? canExecute = null)
    {
        _execute = _ => execute();
        if (canExecute != null) _can = _ => canExecute();
    }

    public RelayCommand(Action<object?> execute, Func<object?, bool>? canExecute = null)
    {
        _execute = execute;
        _can = canExecute;
    }

    public bool CanExecute(object? parameter) => _can == null || _can(parameter);
    public void Execute(object? parameter) { if (CanExecute(parameter)) _execute(parameter); }
    public void RaiseCanExecuteChanged() => CanExecuteChanged?.Invoke();
}

/// <summary>Bindings (Avalonia compiled bindings and reflection bindings).</summary>
public static class BindingExtensions
{
    /// <summary>One-way: the control's property follows <paramref name="getter"/> whenever
    /// <paramref name="source"/> raises PropertyChanged (for <paramref name="path"/>, or any).</summary>
    public static T Bind<T, TValue>(this T control, StyledProperty<TValue> property, INotifyPropertyChanged source,
        Func<TValue> getter, string? path = null) where T : Control
    {
        control.SetValue(property, getter());
        source.PropertyChanged += (_, name) =>
        {
            if (path == null || path == name) control.SetValue(property, getter());
        };
        return control;
    }

    /// <summary>Two-way: like <see cref="Bind{T,TValue}"/>, and a change of the control's property
    /// (typing, dragging, toggling) is written back through <paramref name="setter"/>.</summary>
    public static T BindTwoWay<T, TValue>(this T control, StyledProperty<TValue> property, INotifyPropertyChanged source,
        string path, Func<TValue> getter, Action<TValue> setter) where T : Control
    {
        bool updating = false;
        control.SetValue(property, getter());
        source.PropertyChanged += (_, name) =>
        {
            if (name != path || updating) return;
            updating = true;
            control.SetValue(property, getter());
            updating = false;
        };
        control.PropertyChanged += p =>
        {
            if (p != property || updating) return;
            updating = true;
            setter(control.GetValue(property));
            updating = false;
        };
        return control;
    }

    /// <summary>Reflection binding by name against the DataContext (Avalonia {Binding Path}):
    /// one-way, refreshed on the DataContext's PropertyChanged.</summary>
    public static T BindPath<T>(this T control, StyledProperty<string> property, string path) where T : Control
    {
        void Refresh()
        {
            var dc = control.DataContext;
            if (dc == null) return;
            var pi = dc.GetType().GetProperty(path);
            object? v = pi?.GetValue(dc);
            control.SetValue(property, v?.ToString() ?? "");
        }
        control.DataContextChanged += () =>
        {
            Refresh();
            if (control.DataContext is INotifyPropertyChanged n)
                n.PropertyChanged += (_, name) => { if (name == path) Refresh(); };
        };
        Refresh();
        return control;
    }
}

// CodeEditor -- the text area of Buitenzorg Studio: line numbers, syntax
// colors, the current line, a caret, undo/redo, auto-indent, find/replace and
// go-to-line. It draws only the visible lines, each as a few colored text
// runs (kernel draw ops), never per pixel.

using Buitenzorg.UI;

namespace MStudio;

public sealed class CodeEditor : Control
{
    readonly List<string> _lines = new List<string> { "" };
    int _line, _col;      // caret
    int _top, _left;      // first visible line / column
    int _wantCol = -1;    // column kept while moving up/down
    string _lang = "text";
    bool _caretOn = true;
    long _lastKey;
    DispatcherTimer? _blink;
    // find: the current match
    int _matchLine = -1, _matchCol, _matchLen;

    // undo: snapshots (text, line, col); typing is grouped until a word break
    readonly List<(string, int, int)> _undo = new List<(string, int, int)>();
    readonly List<(string, int, int)> _redo = new List<(string, int, int)>();
    int _lastEdit; // 0 none, 1 insert, 2 delete

    public event Action<CodeEditor>? TextChanged;
    public event Action<CodeEditor>? CaretMoved;

    /// <summary>Lines (1-based) to mark with an error in the gutter.</summary>
    public HashSet<int> ErrorLines { get; } = new HashSet<int>();

    /// <summary>Breakpoint lines (1-based): the open document's own set.</summary>
    public HashSet<int> Breakpoints { get => _bps; set { _bps = value ?? new HashSet<int>(); InvalidateVisual(); } }
    HashSet<int> _bps = new HashSet<int>();

    /// <summary>The line (1-based) the debugger is showing, 0 = none.</summary>
    public int ExecLine { get => _exec; set { _exec = value; InvalidateVisual(); } }
    int _exec;

    public event Action<CodeEditor>? BreakpointsChanged;

    /// <summary>Set or clear the breakpoint on `line` (1-based).</summary>
    public void ToggleBreakpoint(int line)
    {
        if (line < 1 || line > _lines.Count) return;
        if (!_bps.Remove(line)) _bps.Add(line);
        InvalidateVisual();
        BreakpointsChanged?.Invoke(this);
    }
    public int TabSize { get; set; } = 4;

    public CodeEditor()
    {
        Focusable = true;
        AcceptsTab = true;
        ClipToBounds = true;
        MinWidth = 120;
    }

    // ---- content --------------------------------------------------------------

    public string Text
    {
        get => string.Join("\n", _lines);
        set
        {
            SetText(value ?? "");
            _line = _col = _top = _left = 0;
            _undo.Clear();
            _redo.Clear();
            _lastEdit = 0;
            _matchLine = -1;
            InvalidateVisual();
            CaretMoved?.Invoke(this);
        }
    }

    void SetText(string t)
    {
        _lines.Clear();
        foreach (var l in t.Replace("\r\n", "\n").Split('\n')) _lines.Add(l.Replace("\t", new string(' ', TabSize)));
        if (_lines.Count == 0) _lines.Add("");
    }

    public string Language { get => _lang; set { _lang = value; InvalidateVisual(); } }
    public int LineCount => _lines.Count;
    public string LineAt(int i) => _lines[i];
    /// <summary>Caret line, 1-based.</summary>
    public int CaretLine => _line + 1;
    /// <summary>Caret column, 1-based.</summary>
    public int CaretColumn => _col + 1;
    public int FirstVisibleLine => _top;
    public bool CanUndo => _undo.Count > 0;
    public bool CanRedo => _redo.Count > 0;

    // ---- geometry -------------------------------------------------------------

    int Gw => Fonts.GlyphWidth(0);
    int Lh => Fonts.LineHeight(0) + 2;
    int GutterWidth => (Math.Max(3, _lines.Count.ToString().Length) + 2) * Gw;
    int VisibleLines => Math.Max(1, (Bounds.Height - 8) / Lh);
    int VisibleCols => Math.Max(1, (Bounds.Width - GutterWidth - 12) / Gw);

    protected override Size MeasureOverride(Size a) => new Size(Math.Min(a.Width, 480), Math.Min(a.Height, 320));

    void FollowCaret()
    {
        int vis = VisibleLines;
        if (_line < _top) _top = _line;
        if (_line >= _top + vis) _top = _line - vis + 1;
        int cols = VisibleCols;
        if (_col < _left) _left = Math.Max(0, _col - 4);
        if (_col >= _left + cols) _left = _col - cols + 4;
        _top = Math.Max(0, Math.Min(_top, Math.Max(0, _lines.Count - 1)));
    }

    // ---- editing ----------------------------------------------------------------

    void Snapshot(int kind, bool force = false)
    {
        if (force || kind != _lastEdit)
        {
            _undo.Add((Text, _line, _col));
            if (_undo.Count > 200) _undo.RemoveAt(0);
            _redo.Clear();
        }
        _lastEdit = kind;
    }

    void Changed()
    {
        _matchLine = -1;
        if (ErrorLines.Count > 0) ErrorLines.Clear();
        _caretOn = true;
        FollowCaret();
        InvalidateVisual();
        TextChanged?.Invoke(this);
        CaretMoved?.Invoke(this);
    }

    /// <summary>Insert text at the caret (newlines allowed).</summary>
    public void Insert(string s)
    {
        bool wordBreak = s.Length != 1 || s == " " || s == "\n";
        Snapshot(1, wordBreak);
        string cur = _lines[_line];
        string before = cur.Substring(0, _col), after = cur.Substring(_col);
        var parts = s.Replace("\t", new string(' ', TabSize)).Split('\n');
        if (parts.Length == 1)
        {
            _lines[_line] = before + parts[0] + after;
            _col += parts[0].Length;
        }
        else
        {
            _lines[_line] = before + parts[0];
            for (int i = 1; i < parts.Length; i++) _lines.Insert(_line + i, parts[i]);
            _line += parts.Length - 1;
            _col = parts[parts.Length - 1].Length;
            _lines[_line] += after;
        }
        _wantCol = -1;
        Changed();
    }

    string Indent(string line)
    {
        int n = 0;
        while (n < line.Length && line[n] == ' ') n++;
        return line.Substring(0, n);
    }

    /// <summary>Enter: a new line with the current indentation, one level more after
    /// an opening brace (or a ':' in Python), and a closing brace moved down.</summary>
    public void NewLine()
    {
        string cur = _lines[_line];
        string before = cur.Substring(0, _col);
        string after = cur.Substring(_col);
        string ind = Indent(cur);
        string trimmed = before.TrimEnd();
        bool opens = trimmed.EndsWith("{") || trimmed.EndsWith("(") || trimmed.EndsWith("[")
                     || (_lang == "py" && trimmed.EndsWith(":"))
                     || (_lang == "sh" && (trimmed.EndsWith("then") || trimmed.EndsWith("do")));
        string inner = opens ? ind + new string(' ', TabSize) : ind;
        if (opens && after.TrimStart().StartsWith("}"))
            Insert("\n" + inner + "\n" + ind);
        else
            Insert("\n" + inner);
        if (opens && after.TrimStart().StartsWith("}")) { _line--; _col = inner.Length; FollowCaret(); CaretMoved?.Invoke(this); }
    }

    public void Backspace()
    {
        if (_col == 0 && _line == 0) return;
        Snapshot(2);
        if (_col > 0)
        {
            string cur = _lines[_line];
            // inside the indentation: back to the previous tab stop
            int remove = 1;
            if (cur.Substring(0, _col).Trim().Length == 0)
            {
                int stop = (_col - 1) / TabSize * TabSize;
                remove = _col - stop;
            }
            _lines[_line] = cur.Remove(_col - remove, remove);
            _col -= remove;
        }
        else
        {
            _col = _lines[_line - 1].Length;
            _lines[_line - 1] += _lines[_line];
            _lines.RemoveAt(_line);
            _line--;
        }
        _wantCol = -1;
        Changed();
    }

    public void DeleteForward()
    {
        string cur = _lines[_line];
        if (_col < cur.Length) { Snapshot(2); _lines[_line] = cur.Remove(_col, 1); }
        else if (_line + 1 < _lines.Count) { Snapshot(2); _lines[_line] = cur + _lines[_line + 1]; _lines.RemoveAt(_line + 1); }
        else return;
        Changed();
    }

    public void DuplicateLine()
    {
        Snapshot(1, true);
        _lines.Insert(_line + 1, _lines[_line]);
        _line++;
        Changed();
    }

    public void DeleteLine()
    {
        Snapshot(2, true);
        if (_lines.Count == 1) _lines[0] = "";
        else _lines.RemoveAt(_line);
        _line = Math.Min(_line, _lines.Count - 1);
        _col = Math.Min(_col, _lines[_line].Length);
        Changed();
    }

    public bool Undo()
    {
        if (_undo.Count == 0) return false;
        _redo.Add((Text, _line, _col));
        var (t, l, c) = _undo[_undo.Count - 1];
        _undo.RemoveAt(_undo.Count - 1);
        Restore(t, l, c);
        return true;
    }

    public bool Redo()
    {
        if (_redo.Count == 0) return false;
        _undo.Add((Text, _line, _col));
        var (t, l, c) = _redo[_redo.Count - 1];
        _redo.RemoveAt(_redo.Count - 1);
        Restore(t, l, c);
        return true;
    }

    void Restore(string t, int l, int c)
    {
        SetText(t);
        _line = Math.Min(l, _lines.Count - 1);
        _col = Math.Min(c, _lines[_line].Length);
        _lastEdit = 0;
        _matchLine = -1;
        FollowCaret();
        InvalidateVisual();
        TextChanged?.Invoke(this);
        CaretMoved?.Invoke(this);
    }

    // ---- navigation -----------------------------------------------------------------

    public void MoveTo(int line, int col)
    {
        _line = Math.Max(0, Math.Min(line, _lines.Count - 1));
        _col = Math.Max(0, Math.Min(col, _lines[_line].Length));
        _lastEdit = 0;
        _caretOn = true;
        FollowCaret();
        InvalidateVisual();
        CaretMoved?.Invoke(this);
    }

    /// <summary>Go to a 1-based line (its first non-blank character), shown in the middle.</summary>
    public void GoToLine(int line1)
    {
        int l = Math.Max(0, Math.Min(line1 - 1, _lines.Count - 1));
        _top = Math.Max(0, l - VisibleLines / 2);
        MoveTo(l, Indent(_lines[l]).Length);
    }

    /// <summary>Find the next occurrence (case-insensitive) after the caret, wrapping. True if found.</summary>
    public bool FindNext(string needle)
    {
        if (string.IsNullOrEmpty(needle)) return false;
        string n = needle.ToLowerInvariant();
        int startLine = _line, startCol = _matchLine == _line ? _col + 1 : _col;
        for (int k = 0; k <= _lines.Count; k++)
        {
            int li = (startLine + k) % _lines.Count;
            string l = _lines[li].ToLowerInvariant();
            int from = k == 0 ? Math.Min(startCol, l.Length) : 0;
            int at = l.IndexOf(n, from, StringComparison.Ordinal);
            if (at >= 0)
            {
                _matchLine = li; _matchCol = at; _matchLen = needle.Length;
                _top = li < _top || li >= _top + VisibleLines ? Math.Max(0, li - VisibleLines / 2) : _top;
                MoveTo(li, at);
                _matchLine = li;
                return true;
            }
        }
        _matchLine = -1;
        InvalidateVisual();
        return false;
    }

    /// <summary>Replace the current match (found by <see cref="FindNext"/>) and find the next.</summary>
    public bool ReplaceCurrent(string needle, string replacement)
    {
        if (_matchLine != _line || _matchCol != _col || _matchLen != needle.Length) return FindNext(needle);
        Snapshot(1, true);
        string l = _lines[_line];
        _lines[_line] = l.Substring(0, _col) + replacement + l.Substring(_col + _matchLen);
        _col += replacement.Length;
        Changed();
        return FindNext(needle);
    }

    /// <summary>Replace every occurrence (case-insensitive); the count.</summary>
    public int ReplaceAll(string needle, string replacement)
    {
        if (string.IsNullOrEmpty(needle)) return 0;
        Snapshot(1, true);
        int count = 0;
        string n = needle.ToLowerInvariant();
        for (int i = 0; i < _lines.Count; i++)
        {
            string l = _lines[i];
            int at = l.ToLowerInvariant().IndexOf(n, StringComparison.Ordinal);
            while (at >= 0)
            {
                l = l.Substring(0, at) + replacement + l.Substring(at + needle.Length);
                count++;
                int from = at + replacement.Length;
                at = from <= l.Length ? l.ToLowerInvariant().IndexOf(n, from, StringComparison.Ordinal) : -1;
            }
            _lines[i] = l;
        }
        _col = Math.Min(_col, _lines[_line].Length);
        if (count > 0) Changed(); else _undo.RemoveAt(_undo.Count - 1);
        return count;
    }

    void Vertical(int delta)
    {
        if (_wantCol < 0) _wantCol = _col;
        int l = Math.Max(0, Math.Min(_lines.Count - 1, _line + delta));
        int keep = _wantCol;
        MoveTo(l, _wantCol);
        _wantCol = keep;
    }

    // ---- input ------------------------------------------------------------------------

    protected override void OnKeyDown(KeyEventArgs e)
    {
        _lastKey = Clock.Now;
        _blink?.Start();
        char k = e.Key;
        if (k == Keys.Left) { if (_col > 0) MoveTo(_line, _col - 1); else if (_line > 0) MoveTo(_line - 1, _lines[_line - 1].Length); }
        else if (k == Keys.Right) { if (_col < _lines[_line].Length) MoveTo(_line, _col + 1); else if (_line + 1 < _lines.Count) MoveTo(_line + 1, 0); }
        else if (k == Keys.Up) Vertical(-1);
        else if (k == Keys.Down) Vertical(1);
        else if (k == Keys.PageUp) { _top = Math.Max(0, _top - VisibleLines); Vertical(-VisibleLines); }
        else if (k == Keys.PageDown) { _top = Math.Min(Math.Max(0, _lines.Count - 1), _top + VisibleLines); Vertical(VisibleLines); }
        else if (k == Keys.Home || k == (char)1)
        {
            // first non-blank, then column 0
            int ind = Indent(_lines[_line]).Length;
            MoveTo(_line, _col == ind ? 0 : ind);
        }
        else if (k == Keys.End || k == (char)5) MoveTo(_line, _lines[_line].Length);
        else if (k == Keys.Backspace) Backspace();
        else if (k == Keys.Delete) DeleteForward();
        else if (k == Keys.Enter) NewLine();
        else if (k == Keys.Tab) Insert(new string(' ', TabSize - _col % TabSize));
        else if (k == (char)26) Undo();            // Ctrl+Z
        else if (k == (char)25) Redo();            // Ctrl+Y
        else if (k == (char)4) DuplicateLine();    // Ctrl+D
        else if (k == (char)11) DeleteLine();      // Ctrl+K
        else if (Keys.IsPrintable(k))
        {
            Insert(k.ToString());
            // a closing brace on an otherwise blank line goes one level back
            string cur = _lines[_line];
            if ((k == '}' || k == ']' || k == ')') && cur.Trim() == k.ToString() && _line > 0)
            {
                string open = Indent(_lines[_line - 1]);
                string want = Indent(cur).Length >= TabSize && !_lines[_line - 1].TrimEnd().EndsWith("{") ? Indent(cur).Substring(TabSize) : open;
                if (_lines[_line - 1].TrimEnd().EndsWith("{")) want = open;
                _lines[_line] = want + k;
                _col = _lines[_line].Length;
                Changed();
            }
        }
        else { base.OnKeyDown(e); return; }
        e.Handled = true;
    }

    protected override void OnPointerPressed(PointerEventArgs e)
    {
        Focus();
        var r = Bounds;
        int line = _top + Math.Max(0, (e.Y - r.Y - 4) / Lh);
        int col = _left + Math.Max(0, (e.X - r.X - GutterWidth - 6 + Gw / 2) / Gw);
        if (line >= _lines.Count) line = _lines.Count - 1;
        if (e.X < r.X + GutterWidth)
        {
            ToggleBreakpoint(line + 1); // a click in the gutter: breakpoint
            e.Handled = true;
            return;
        }
        MoveTo(line, col);
        e.Handled = true;
        base.OnPointerPressed(e);
    }

    protected override void OnPointerWheelChanged(PointerEventArgs e)
    {
        int max = Math.Max(0, _lines.Count - VisibleLines);
        int next = Math.Max(0, Math.Min(max, _top - e.WheelDelta * 3));
        if (next != _top) { _top = next; e.Handled = true; InvalidateVisual(); }
        base.OnPointerWheelChanged(e);
    }

    protected override void OnGotFocus()
    {
        base.OnGotFocus();
        _blink ??= new DispatcherTimer(530);
        _blink.Tick += Blink;
        _blink.Start();
        _caretOn = true;
        _lastKey = Clock.Now;
    }

    protected override void OnLostFocus()
    {
        base.OnLostFocus();
        if (_blink != null) { _blink.Tick -= Blink; _blink.Stop(); }
        InvalidateVisual();
    }

    // like TextBox: an idle caret stops blinking (each blink recomposes the desktop)
    void Blink()
    {
        if (Clock.Now - _lastKey > 10000) { _caretOn = true; _blink?.Stop(); }
        else _caretOn = !_caretOn;
        InvalidateVisual();
    }

    // ---- drawing ------------------------------------------------------------------------

    public override void Render(DrawingContext dc)
    {
        var r = Bounds;
        bool dark = Application.Current == null || Application.Current.ThemeVariant == ThemeVariant.Dark;
        int bg = dark ? Buitenzorg.UI.Color.Darken(Theme.Surface, 60) : Buitenzorg.UI.Color.Lighten(Theme.Surface, 120);
        int gutterBg = dark ? Buitenzorg.UI.Color.Darken(Theme.Surface, 25) : Theme.Surface;
        int gw = Gw, lh = Lh, gutter = GutterWidth;
        dc.FillRect(r, bg);
        dc.FillRect(new Rect(r.X, r.Y, gutter, r.Height), gutterBg);
        int vis = VisibleLines;
        bool inBlock = Syntax.StateBefore(_lines, _top, _lang);
        int textX = r.X + gutter + 6;
        int cols = VisibleCols;
        dc.PushClip(r);
        for (int i = _top; i < _lines.Count && i < _top + vis + 1; i++)
        {
            int y = r.Y + 4 + (i - _top) * lh;
            if (i == _line && IsFocused)
                dc.FillRect(new Rect(r.X + gutter, y - 1, r.Width - gutter, lh), dark ? Buitenzorg.UI.Color.Lighten(bg, 12) : Buitenzorg.UI.Color.Darken(bg, 10));
            if (i + 1 == _exec)
                dc.FillRect(new Rect(r.X + gutter, y - 1, r.Width - gutter, lh), dark ? 0x4A4214 : 0xFFF1A8);
            if (i == _matchLine)
            {
                int mx = textX + (_matchCol - _left) * gw;
                dc.FillRect(new Rect(mx, y - 1, _matchLen * gw, lh), dark ? 0x5A4A1A : 0xF2E19A);
            }
            // line number (an error line gets a red mark)
            string num = (i + 1).ToString();
            bool err = ErrorLines.Contains(i + 1);
            if (err) dc.FillRect(new Rect(r.X + 2, y + 1, 4, lh - 4), Theme.Danger);
            if (_bps.Contains(i + 1)) dc.Ellipse(new Rect(r.X + 3, y + (lh - 10) / 2, 10, 10), Theme.Danger, true);
            if (i + 1 == _exec) dc.Text(r.X + 2, y, ">", Theme.Accent, 1);
            dc.Text(r.X + gutter - (num.Length + 1) * gw, y, num, err ? Theme.Danger : i == _line ? Theme.Text : Theme.TextFaint, 0);
            // the line's colored runs, cut to the visible columns
            string line = _lines[i];
            foreach (var s in Syntax.Line(line, _lang, ref inBlock))
            {
                if (s.Kind == Tk.Plain && line.Substring(s.Start, s.Length).Trim().Length == 0) continue;
                int a = Math.Max(s.Start, _left), b = Math.Min(s.Start + s.Length, _left + cols);
                if (b <= a) continue;
                dc.Text(textX + (a - _left) * gw, y, line.Substring(a, b - a), Syntax.Color(s.Kind, dark, Theme.Text), 0);
            }
        }
        if (IsFocused && _caretOn)
        {
            int cx = textX + (_col - _left) * gw;
            int cy = r.Y + 4 + (_line - _top) * lh;
            if (_line >= _top && _line < _top + vis + 1) dc.FillRect(new Rect(cx, cy, 2, lh - 2), Theme.Accent);
        }
        dc.PopClip();
    }
}

// Kartu -- patience (Klondike rules): build the four foundations up from Ace
// to King by suit; on the seven columns, build down in alternating colours.
// Click a card (or the top of a run) to pick it up, click where it goes; a
// second click on the same card sends it to a foundation. Click the stock to
// turn a card. N = new deal, U = undo. Buitenzorg.Game.

using Buitenzorg.Game;
using Buitenzorg.UI;

namespace MKartu;

public readonly struct Card
{
    public readonly int Suit;  // 0 spades, 1 hearts, 2 diamonds, 3 clubs
    public readonly int Rank;  // 1 ace .. 13 king
    public Card(int suit, int rank) { Suit = suit; Rank = rank; }
    public bool Red => Suit == 1 || Suit == 2;
    public override string ToString() => RankText(Rank) + "SHDC"[Suit];
    public static string RankText(int r) => r switch { 1 => "A", 11 => "J", 12 => "Q", 13 => "K", _ => r.ToString() };
}

/// <summary>Piles: 0 stock, 1 waste, 2..5 foundations, 6..12 columns.</summary>
public sealed class Table
{
    public const int Stock = 0, Waste = 1, Found0 = 2, Col0 = 6;
    public readonly List<Card>[] Piles = new List<Card>[13];
    /// <summary>How many cards of each column are face down (from the bottom).</summary>
    public readonly int[] Hidden = new int[13];
    public int Moves { get; private set; }
    readonly Stack<(List<Card>[] Piles, int[] Hidden, int Moves)> _undo = new Stack<(List<Card>[], int[], int)>();

    public Table(ulong seed)
    {
        for (int i = 0; i < 13; i++) Piles[i] = new List<Card>();
        var deck = new List<Card>();
        for (int s = 0; s < 4; s++) for (int r = 1; r <= 13; r++) deck.Add(new Card(s, r));
        new Rng(seed).Shuffle(deck);
        int k = 0;
        for (int c = 0; c < 7; c++)
        {
            for (int n = 0; n <= c; n++) Piles[Col0 + c].Add(deck[k++]);
            Hidden[Col0 + c] = c;
        }
        while (k < 52) Piles[Stock].Add(deck[k++]);
    }

    public bool Won { get { for (int f = 0; f < 4; f++) if (Piles[Found0 + f].Count != 13) return false; return true; } }

    void Save()
    {
        var copy = new List<Card>[13];
        for (int i = 0; i < 13; i++) copy[i] = new List<Card>(Piles[i]);
        _undo.Push((copy, (int[])Hidden.Clone(), Moves));
        if (_undo.Count > 200) { var keep = _undo.ToArray(); _undo.Clear(); for (int i = 99; i >= 0; i--) _undo.Push(keep[i]); }
    }

    public bool Undo()
    {
        if (_undo.Count == 0) return false;
        var (p, h, m) = _undo.Pop();
        for (int i = 0; i < 13; i++) Piles[i] = p[i];
        Array.Copy(h, Hidden, 13);
        Moves = m;
        return true;
    }

    /// <summary>Turn one card from the stock, or put the waste back.</summary>
    public void Draw()
    {
        Save();
        if (Piles[Stock].Count > 0)
        {
            var c = Piles[Stock][^1];
            Piles[Stock].RemoveAt(Piles[Stock].Count - 1);
            Piles[Waste].Add(c);
        }
        else
        {
            for (int i = Piles[Waste].Count - 1; i >= 0; i--) Piles[Stock].Add(Piles[Waste][i]);
            Piles[Waste].Clear();
        }
        Moves++;
    }

    public bool CanFoundation(Card c, int f)
    {
        var p = Piles[Found0 + f];
        return p.Count == 0 ? c.Rank == 1 : p[^1].Suit == c.Suit && p[^1].Rank == c.Rank - 1;
    }

    public bool CanColumn(Card c, int col)
    {
        var p = Piles[Col0 + col];
        if (p.Count == 0) return c.Rank == 13;
        var top = p[^1];
        return top.Red != c.Red && top.Rank == c.Rank + 1;
    }

    /// <summary>The first movable index of a pile for a pick at `index` (columns: face-up runs).</summary>
    public bool CanPick(int pile, int index)
    {
        var p = Piles[pile];
        if (index < 0 || index >= p.Count) return false;
        if (pile == Waste || (pile >= Found0 && pile < Col0)) return index == p.Count - 1;
        if (pile >= Col0) return index >= Hidden[pile];
        return false;
    }

    /// <summary>Move the cards from `index` of `from` onto pile `to`.</summary>
    public bool Move(int from, int index, int to)
    {
        if (!CanPick(from, index) || from == to) return false;
        var run = Piles[from].GetRange(index, Piles[from].Count - index);
        bool ok = to >= Col0 ? CanColumn(run[0], to - Col0) : to >= Found0 && to < Col0 && run.Count == 1 && CanFoundation(run[0], to - Found0);
        if (!ok) return false;
        Save();
        Piles[from].RemoveRange(index, run.Count);
        Piles[to].AddRange(run);
        if (from >= Col0 && Hidden[from] >= Piles[from].Count && Hidden[from] > 0) Hidden[from] = Piles[from].Count - 1;
        if (from >= Col0 && Piles[from].Count == 0) Hidden[from] = 0;
        Moves++;
        return true;
    }

    /// <summary>Send the top card of a pile to whichever foundation takes it.</summary>
    public bool ToFoundation(int from)
    {
        var p = Piles[from];
        if (p.Count == 0 || (from >= Col0 && p.Count - 1 < Hidden[from])) return false;
        for (int f = 0; f < 4; f++) if (CanFoundation(p[^1], f)) return Move(from, p.Count - 1, Found0 + f);
        return false;
    }

    /// <summary>Everything face up and the stock empty: move all to the foundations.</summary>
    public bool AutoFinish()
    {
        if (Piles[Stock].Count > 0 || Piles[Waste].Count > 0) return false;
        for (int c = 0; c < 7; c++) if (Hidden[Col0 + c] > 0) return false;
        bool moved = true;
        while (moved && !Won)
        {
            moved = false;
            for (int c = 0; c < 7; c++) moved |= ToFoundation(Col0 + c);
        }
        return Won;
    }
}

public sealed class KartuView : GameView
{
    public Table Table { get; private set; }
    public (int Pile, int Index)? Held { get; private set; }
    readonly ulong _seed;
    readonly HighScores _best;
    readonly Particles _fx = new Particles();
    readonly Rng _rng = new Rng(41);
    int _deals;
    public event Action? Changed;
    public const int CW = 60, CH = 84, Gap = 8;

    public KartuView(ulong seed, HighScores best) : base(7 * (CW + Gap) + Gap, 420, 30) { _seed = seed; _best = best; Table = new Table(seed); }

    public void NewDeal() { Table = new Table(_seed + (ulong)(++_deals) * 7919); Held = null; _fx.Clear(); Changed?.Invoke(); }

    protected override void Update()
    {
        if (Input.WasPressed(RawKey.N)) NewDeal();
        if (Input.WasPressed(RawKey.U)) { Table.Undo(); Held = null; Changed?.Invoke(); }
        if (Input.WasPressed(RawKey.Escape)) Held = null;
        _fx.Update(Dt, 120);
    }

    public static double PileX(int pile) => pile switch
    {
        Table.Stock => Gap,
        Table.Waste => Gap + (CW + Gap),
        < Table.Col0 => Gap + (pile - Table.Found0 + 3) * (CW + Gap),
        _ => Gap + (pile - Table.Col0) * (CW + Gap),
    };
    public const double TopY = 10, ColY = 110, FanDown = 10, FanUp = 22;

    /// <summary>The y of card `i` in a column.</summary>
    public double CardY(int pile, int i)
    {
        if (pile < Table.Col0) return TopY;
        double y = ColY;
        for (int k = 0; k < i; k++) y += k < Table.Hidden[pile] ? FanDown : FanUp;
        return y;
    }

    /// <summary>The pile and card index at a point in game units (index -1 = the empty pile).</summary>
    public (int Pile, int Index)? HitTest(double gx, double gy)
    {
        for (int pile = 0; pile < 13; pile++)
        {
            double x = PileX(pile);
            if (gx < x || gx >= x + CW) continue;
            var p = Table.Piles[pile];
            if (pile < Table.Col0)
            {
                if (gy >= TopY && gy < TopY + CH) return (pile, p.Count - 1);
                continue;
            }
            for (int i = p.Count - 1; i >= 0; i--)
            {
                double y = CardY(pile, i);
                if (gy >= y && gy < y + CH) return (pile, i);
            }
            if (p.Count == 0 && gy >= ColY && gy < ColY + CH) return (pile, -1);
        }
        return null;
    }

    /// <summary>A click on (pile, index): pick up, put down, or send home.</summary>
    public void Click(int pile, int index)
    {
        if (pile == Table.Stock) { Table.Draw(); Held = null; Sfx.Blip(); Changed?.Invoke(); return; }
        if (Held is (int hp, int hi))
        {
            if (hp == pile && (hi == index || index == Table.Piles[pile].Count - 1))
            {
                // the same card again: to a foundation
                if (hi == Table.Piles[hp].Count - 1 && Table.ToFoundation(hp)) Moved();
                Held = null;
                return;
            }
            if (Table.Move(hp, hi, pile)) Moved(); else Sfx.Hit();
            Held = null;
            return;
        }
        if (index >= 0 && Table.CanPick(pile, index)) Held = (pile, index);
    }

    void Moved()
    {
        Sfx.Blip();
        if (Table.AutoFinish() || Table.Won)
        {
            Sfx.Score();
            _best.Add(Math.Max(1, 1000 - Table.Moves), save: GameEnv.Interactive);
            _fx.Burst(_rng, FieldWidth / 2.0, 200, 80, 0xF2B544, 260, 1.5, 4);
        }
        Changed?.Invoke();
    }

    protected override void OnPointerPressed(PointerEventArgs e)
    {
        Focus();
        var f = FieldRect;
        double gx = (e.X - f.X) * (double)FieldWidth / f.Width, gy = (e.Y - f.Y) * (double)FieldHeight / f.Height;
        if (HitTest(gx, gy) is (int p, int i)) { Click(p, i); InvalidateVisual(); }
        else Held = null;
        e.Handled = true;
    }

    protected override void Draw(DrawingContext dc, Rect f)
    {
        dc.FillGradient(f, 0, 0x1F5A36, 0x17462A);
        for (int pile = 0; pile < 13; pile++)
        {
            if (pile >= Table.Found0 && pile < Table.Col0 || pile == Table.Stock || pile >= Table.Col0)
                dc.StrokeRoundRect(SRect(PileX(pile), pile >= Table.Col0 ? ColY : TopY, CW, CH), Math.Max(2, Sl(6)), 0x2E7148, 2);
            var p = Table.Piles[pile];
            if (pile == Table.Stock) { if (p.Count > 0) Back(dc, PileX(pile), TopY); continue; }
            int from = pile < Table.Col0 ? Math.Max(0, p.Count - 1) : 0;
            for (int i = from; i < p.Count; i++)
            {
                double x = PileX(pile), y = CardY(pile, i);
                if (pile >= Table.Col0 && i < Table.Hidden[pile]) { Back(dc, x, y); continue; }
                bool held = Held is (int hp, int hi) && hp == pile && i >= hi;
                Face(dc, p[i], x, y, held);
            }
        }
        _fx.Draw(dc, f.X, f.Y);
        if (Table.Won) Banner(dc, f, "Semua kartu pulang!", Table.Moves + " langkah  -  N untuk kartu baru");
    }

    void Back(DrawingContext dc, double x, double y)
    {
        var r = SRect(x, y, CW, CH);
        dc.FillGradient(r, Math.Max(2, Sl(6)), 0x3B7FB8, 0x2A5C88);
        dc.StrokeRoundRect(SRect(x + 4, y + 4, CW - 8, CH - 8), Math.Max(2, Sl(4)), 0x9CC4E6, 1);
    }

    void Face(DrawingContext dc, Card c, double x, double y, bool held)
    {
        var r = SRect(x, y, CW, CH);
        dc.FillRoundRect(r, Math.Max(2, Sl(6)), held ? 0xFFF4C8 : 0xF8F6EE);
        dc.StrokeRoundRect(r, Math.Max(2, Sl(6)), held ? 0xF2B544 : 0xB9B4A4, held ? 2 : 1);
        int ink = c.Red ? 0xC8352B : 0x1E2A24;
        dc.Text(Sx(x + 5), Sy(y + 3), Card.RankText(c.Rank), ink, 1);
        Pip(dc, x + CW - 14, y + 12, 8, c.Suit, ink);
        Pip(dc, x + CW / 2.0, y + CH / 2.0 + 6, 18, c.Suit, ink);
    }

    /// <summary>A suit symbol from primitives (the font has no card symbols).</summary>
    void Pip(DrawingContext dc, double cx, double cy, double s, int suit, int ink)
    {
        switch (suit)
        {
            case 2: // diamond: rows of shrinking width
                for (double k = -s / 2; k <= s / 2; k += 1)
                {
                    double half = (s / 2 - Math.Abs(k)) * 0.75;
                    dc.FillRect(SRect(cx - half, cy + k, half * 2 + 1, 1), ink);
                }
                break;
            case 1: // heart: two lobes and a point
                dc.Ellipse(SRect(cx - s / 2, cy - s / 2, s / 2 + 1, s / 2 + 1), ink, true);
                dc.Ellipse(SRect(cx, cy - s / 2, s / 2 + 1, s / 2 + 1), ink, true);
                for (double k = 0; k <= s / 2; k += 1)
                {
                    double half = (s / 2) * (1 - k / (s / 2));
                    dc.FillRect(SRect(cx - half, cy - s / 8 + k, half * 2 + 1, 1), ink);
                }
                break;
            case 0: // spade: an upside-down heart and a stem
                dc.Ellipse(SRect(cx - s / 2, cy - s / 8, s / 2 + 1, s / 2 + 1), ink, true);
                dc.Ellipse(SRect(cx, cy - s / 8, s / 2 + 1, s / 2 + 1), ink, true);
                for (double k = 0; k <= s / 2; k += 1)
                {
                    double half = (s / 2) * (k / (s / 2));
                    dc.FillRect(SRect(cx - half, cy - s / 2 + k, half * 2 + 1, 1), ink);
                }
                dc.FillRect(SRect(cx - s / 12, cy + s / 4, Math.Max(1, s / 6), s / 3), ink);
                break;
            default: // club: three leaves and a stem
                double l = s * 0.42;
                dc.Ellipse(SRect(cx - l / 2, cy - s / 2, l, l), ink, true);
                dc.Ellipse(SRect(cx - s / 2, cy - s / 8, l, l), ink, true);
                dc.Ellipse(SRect(cx + s / 2 - l, cy - s / 8, l, l), ink, true);
                dc.FillRect(SRect(cx - s / 12, cy, Math.Max(1, s / 6), s / 2), ink);
                break;
        }
    }
}

public static class KartuApp
{
    public static (Window, KartuView) Build(bool headless = false, ulong seed = 0)
    {
        var best = new HighScores("KARTU", load: !headless);
        var view = new KartuView(seed == 0 ? (ulong)Clock.Now | 1 : seed, best);
        var status = new TextBlock() { Margin = new Thickness(4, 0, 4, 8) };
        void Refresh() => status.Text = "Langkah " + view.Table.Moves + "    Di fondasi " + Enumerable.Range(0, 4).Sum(f => view.Table.Piles[Table.Found0 + f].Count) + "/52    (klik pilih, klik tujuan; N baru, U batal)";
        view.Changed += Refresh;
        Refresh();
        var w = GameHost.Build("Kartu", view, 540, 520, status, headless);
        if (!headless) view.Start();
        return (w, view);
    }

    public static Checks SelfTest()
    {
        var c = new Checks("Kartu");
        var t = new Table(2024);
        int total = t.Piles.Sum(p => p.Count);
        var all = t.Piles.SelectMany(p => p).Select(x => x.Suit * 13 + x.Rank).Distinct().Count();
        c.That(total == 52 && all == 52 && t.Piles[Table.Col0 + 6].Count == 7 && t.Piles[Table.Stock].Count == 24, "a deal: 52 distinct cards, columns 1..7, 24 in the stock");
        c.That(t.CanColumn(new Card(1, 12), 0) == (t.Piles[Table.Col0][^1].Rank == 13 && !t.Piles[Table.Col0][^1].Red), "alternating colours, one rank down");
        c.That(t.CanFoundation(new Card(0, 1), 0) && !t.CanFoundation(new Card(0, 2), 0), "a foundation starts with an ace");
        t.Draw();
        c.That(t.Piles[Table.Waste].Count == 1 && t.Piles[Table.Stock].Count == 23, "the stock turns a card onto the waste");
        for (int i = 0; i < 23; i++) t.Draw();
        t.Draw();
        c.That(t.Piles[Table.Stock].Count == 24 && t.Piles[Table.Waste].Count == 0, "an empty stock takes the waste back");
        c.That(t.Undo() && t.Piles[Table.Waste].Count == 24, "undo puts it back");
        // a constructed position: a black 5 on a red 6, then to the foundation chain
        var u = new Table(1);
        for (int i = 0; i < 13; i++) u.Piles[i].Clear();
        Array.Clear(u.Hidden, 0, 13);
        u.Piles[Table.Col0].Add(new Card(1, 6));
        u.Piles[Table.Col0 + 1].Add(new Card(0, 5));
        c.That(u.Move(Table.Col0 + 1, 0, Table.Col0) && u.Piles[Table.Col0].Count == 2, "a black 5 goes onto a red 6");
        c.That(!u.Move(Table.Col0, 1, Table.Col0 + 2) && u.Piles[Table.Col0 + 2].Count == 0, "a 5 does not go on an empty column (kings only)");
        // auto-finish: everything face up, nothing in the stock
        var v = new Table(1);
        for (int i = 0; i < 13; i++) v.Piles[i].Clear();
        Array.Clear(v.Hidden, 0, 13);
        for (int s = 0; s < 4; s++) for (int r = 13; r >= 1; r--) v.Piles[Table.Col0 + s].Add(new Card(s, r));
        c.That(v.AutoFinish() && v.Won, "auto-finish sends everything home");

        var (w, view) = Build(headless: true, seed: 3);
        var fr = view.FieldRect;
        int stock = view.Table.Piles[Table.Stock].Count;
        w.SimulatePointer(1, view.Sx(KartuView.PileX(Table.Stock) + 10), view.Sy(KartuView.TopY + 10));
        w.SimulatePointer(2, view.Sx(KartuView.PileX(Table.Stock) + 10), view.Sy(KartuView.TopY + 10));
        w.SimulatePointer(3, view.Sx(KartuView.PileX(Table.Stock) + 10), view.Sy(KartuView.TopY + 10));
        c.That(view.Table.Piles[Table.Stock].Count == stock - 1, "a click on the stock turns a card");
        w.RenderFrame();
        c.That(w.LastFrameOps > 100, "the table drew " + w.LastFrameOps + " ops");
        return c;
    }
}

public static class Program
{
    public static int Main(string[] args)
    {
        var app = new Application();
        if (Array.IndexOf(args, "--selftest") >= 0)
        {
            var c = KartuApp.SelfTest();
            Console.WriteLine(c.ToString());
            return c.Ok ? 0 : 1;
        }
        var (w, _) = KartuApp.Build();
        app.Run(w);
        return 0;
    }
}

// XOX (Tic-Tac-Toe) — Buitenzorg desktop app template (requirements.md §13.2).
// A third-party-style C# app: it creates a window and draws its UI through the
// Buitenzorg window syscalls (WIN_CREATE / WIN_CMD / WIN_PRESENT), compiled
// freestanding with bflat (--stdlib:zero) + the bzstart shim.
//
// This build plays a short scripted game (X wins on a diagonal) so it is
// verifiable headless; the same drawing code backs an interactive version
// driven by bz_key_read (1..9 to place a mark).

using System;
using System.Runtime.InteropServices;

static unsafe class Bz
{
    [DllImport("*")] public static extern unsafe uint bz_win_create(byte* title, ulong len, ulong dims);
    [DllImport("*")] public static extern unsafe ulong bz_win_cmd(uint window, DrawCmd* cmd);
    [DllImport("*")] public static extern void bz_win_present(uint window);
    [DllImport("*")] public static extern ulong bz_ticks();

    // Mirror of bz_abi::DrawCmd (48 bytes).
    public struct DrawCmd
    {
        public ulong Op;
        public int X, Y, W, H;
        public uint Color, Pad;
        public ulong TextPtr, TextLen;
    }

    public const ulong OpFill = 0, OpText = 1, OpClear = 2;

    public static uint CreateWindow(string title, int w, int h)
    {
        byte* buf = stackalloc byte[64];
        int n = 0;
        fixed (char* tc = title)
            for (int i = 0; i < title.Length && n < 63; i++)
                buf[n++] = (byte)tc[i];
        return bz_win_create(buf, (ulong)n, ((ulong)(uint)w << 32) | (uint)h);
    }

    public static void Fill(uint win, int x, int y, int w, int h, uint color)
    {
        var cmd = new DrawCmd { Op = OpFill, X = x, Y = y, W = w, H = h, Color = color };
        bz_win_cmd(win, &cmd);
    }

    public static void ClearWin(uint win, uint color)
    {
        var cmd = new DrawCmd { Op = OpClear, Color = color };
        bz_win_cmd(win, &cmd);
    }

    public static void Text(uint win, int x, int y, string s, uint color)
    {
        byte* buf = stackalloc byte[128];
        int n = 0;
        fixed (char* sc = s)
            for (int i = 0; i < s.Length && n < 127; i++)
                buf[n++] = (byte)sc[i];
        var cmd = new DrawCmd { Op = OpText, X = x, Y = y, Color = color, TextPtr = (ulong)buf, TextLen = (ulong)n };
        bz_win_cmd(win, &cmd);
    }

    public static void Present(uint win) => bz_win_present(win);

    public static void Sleep(ulong ticks)
    {
        ulong until = bz_ticks() + ticks;
        while (bz_ticks() < until) { }
    }
}

// Freestanding (no GC): the board lives on the stack, no heap arrays are used.
unsafe class Xox
{
    const uint Bg = 0x141C16;
    const uint Grid = 0x4FA33F;
    const uint XColor = 0x6FC14E;
    const uint OColor = 0xE8B84B;
    const uint TextColor = 0xC8E9B0;
    const int Cell = 90;
    const int Margin = 10;

    static void Main()
    {
        Console.WriteLine("[xox] starting (C# desktop app, ring 3)");
        uint win = Bz.CreateWindow("XOX - Tic Tac Toe", 3 * Cell + 2 * Margin, 3 * Cell + 2 * Margin + 30);

        char* board = stackalloc char[9];
        for (int i = 0; i < 9; i++) board[i] = ' ';
        DrawBoard(win, board, "Ayo main!");

        // Scripted game: X takes the main diagonal and wins.
        int* moves = stackalloc int[5];
        moves[0] = 0; moves[1] = 1; moves[2] = 4; moves[3] = 2; moves[4] = 8;
        char turn = 'X';
        for (int i = 0; i < 5; i++)
        {
            board[moves[i]] = turn;
            DrawBoard(win, board, turn == 'X' ? "X jalan" : "O jalan");
            Bz.Sleep(9); // ~0.5s per move
            turn = turn == 'X' ? 'O' : 'X';
        }

        DrawBoard(win, board, "X menang! (diagonal)");
        Console.WriteLine("[xox] game over: X wins");
        Bz.Sleep(18);
    }

    static void DrawBoard(uint win, char* board, string status)
    {
        Bz.ClearWin(win, Bg);
        for (int i = 1; i < 3; i++)
        {
            Bz.Fill(win, Margin + i * Cell - 1, Margin, 2, 3 * Cell, Grid);
            Bz.Fill(win, Margin, Margin + i * Cell - 1, 3 * Cell, 2, Grid);
        }
        for (int r = 0; r < 3; r++)
            for (int c = 0; c < 3; c++)
            {
                char m = board[r * 3 + c];
                if (m == ' ') continue;
                int cx = Margin + c * Cell + Cell / 2 - 4;
                int cy = Margin + r * Cell + Cell / 2 - 8;
                Bz.Text(win, cx, cy, m == 'X' ? "X" : "O", m == 'X' ? XColor : OColor);
            }
        Bz.Text(win, Margin, 3 * Cell + Margin + 8, status, TextColor);
        Bz.Present(win);
    }
}

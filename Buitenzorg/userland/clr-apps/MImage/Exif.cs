using System;
using System.Collections.Generic;
using System.Text;

namespace MImage;

/// <summary>EXIF data of a photo: camera, date, exposure, orientation and GPS,
/// from a JPEG's APP1 "Exif" segment or a PNG's eXIf chunk (TIFF structure,
/// either byte order). Unknown tags are skipped.</summary>
public sealed class Exif
{
    public string Make = "", Model = "", Software = "", DateTime = "", Lens = "";
    /// <summary>1 = as stored; 3 = turned 180; 6 = turn 90 clockwise; 8 = turn 90 anticlockwise; 2/4/5/7 mirrored.</summary>
    public int Orientation = 1;
    public double ExposureSeconds, FNumber, FocalMm;
    public int Iso, PixelWidth, PixelHeight;
    public bool HasGps;
    public double Latitude, Longitude;
    public int Tags;

    byte[] _d = Array.Empty<byte>();
    int _base;
    bool _le;

    /// <summary>The EXIF block of a JPEG or PNG file, or null.</summary>
    public static Exif? FromFile(byte[] d)
    {
        if (d.Length > 4 && d[0] == 0xFF && d[1] == 0xD8)
        {
            int p = 2;
            while (p + 4 <= d.Length && d[p] == 0xFF)
            {
                int marker = d[p + 1];
                if (marker == 0xD9 || marker == 0xDA) break; // end / start of scan: no more metadata
                int len = (d[p + 2] << 8) | d[p + 3];
                if (len < 2) break;
                if (marker == 0xE1 && len >= 8 && p + 4 + 6 <= d.Length && d[p + 4] == 'E' && d[p + 5] == 'x' && d[p + 6] == 'i' && d[p + 7] == 'f' && d[p + 8] == 0)
                    return Parse(d, p + 10, Math.Min(d.Length, p + 2 + len));
                p += 2 + len;
            }
            return null;
        }
        if (d.Length > 8 && d[0] == 0x89 && d[1] == 'P' && d[2] == 'N' && d[3] == 'G')
        {
            int p = 8;
            while (p + 8 <= d.Length)
            {
                int len = (d[p] << 24) | (d[p + 1] << 16) | (d[p + 2] << 8) | d[p + 3];
                if (len < 0 || p + 12 + len > d.Length) break;
                if (d[p + 4] == 'e' && d[p + 5] == 'X' && d[p + 6] == 'I' && d[p + 7] == 'f') return Parse(d, p + 8, p + 8 + len);
                if (d[p + 4] == 'I' && d[p + 5] == 'D' && d[p + 6] == 'A' && d[p + 7] == 'T') break;
                p += 12 + len;
            }
        }
        return null;
    }

    /// <summary>A TIFF structure at d[start..end).</summary>
    public static Exif? Parse(byte[] d, int start, int end)
    {
        if (end - start < 8) return null;
        var e = new Exif { _d = d, _base = start };
        if (d[start] == 'I' && d[start + 1] == 'I') e._le = true;
        else if (d[start] != 'M' || d[start + 1] != 'M') return null;
        if (e.U16(2) != 42) return null;
        int ifd0 = (int)e.U32(4);
        int exifIfd = 0, gpsIfd = 0;
        e.ReadIfd(ifd0, end - start, (tag, type, count, off) =>
        {
            switch (tag)
            {
                case 0x010F: e.Make = e.Ascii(off, count); break;
                case 0x0110: e.Model = e.Ascii(off, count); break;
                case 0x0112: e.Orientation = (int)e.Value(type, off); break;
                case 0x0131: e.Software = e.Ascii(off, count); break;
                case 0x0132: if (e.DateTime.Length == 0) e.DateTime = e.Ascii(off, count); break;
                case 0x8769: exifIfd = (int)e.Value(type, off); break;
                case 0x8825: gpsIfd = (int)e.Value(type, off); break;
            }
        });
        if (exifIfd > 0)
            e.ReadIfd(exifIfd, end - start, (tag, type, count, off) =>
            {
                switch (tag)
                {
                    case 0x829A: e.ExposureSeconds = e.Rational(e.Ptr(off, type, count)); break;
                    case 0x829D: e.FNumber = e.Rational(e.Ptr(off, type, count)); break;
                    case 0x8827: e.Iso = (int)e.Value(type, off); break;
                    case 0x9003: e.DateTime = e.Ascii(off, count); break;
                    case 0x920A: e.FocalMm = e.Rational(e.Ptr(off, type, count)); break;
                    case 0xA002: e.PixelWidth = (int)e.Value(type, off); break;
                    case 0xA003: e.PixelHeight = (int)e.Value(type, off); break;
                    case 0xA434: e.Lens = e.Ascii(off, count); break;
                }
            });
        if (gpsIfd > 0)
        {
            char latRef = 'N', lonRef = 'E';
            double lat = -1, lon = -1;
            e.ReadIfd(gpsIfd, end - start, (tag, type, count, off) =>
            {
                switch (tag)
                {
                    case 1: latRef = (char)e._d[e._base + off]; break;
                    case 2: lat = e.Degrees(e.Ptr(off, type, count)); break;
                    case 3: lonRef = (char)e._d[e._base + off]; break;
                    case 4: lon = e.Degrees(e.Ptr(off, type, count)); break;
                }
            });
            if (lat >= 0 && lon >= 0)
            {
                e.HasGps = true;
                e.Latitude = latRef == 'S' ? -lat : lat;
                e.Longitude = lonRef == 'W' ? -lon : lon;
            }
        }
        if (e.Orientation < 1 || e.Orientation > 8) e.Orientation = 1;
        e._d = Array.Empty<byte>();
        return e;
    }

    int U16(int o) => _le ? _d[_base + o] | (_d[_base + o + 1] << 8) : (_d[_base + o] << 8) | _d[_base + o + 1];
    long U32(int o) => _le
        ? (long)(uint)(_d[_base + o] | (_d[_base + o + 1] << 8) | (_d[_base + o + 2] << 16) | (_d[_base + o + 3] << 24))
        : (long)(uint)((_d[_base + o] << 24) | (_d[_base + o + 1] << 16) | (_d[_base + o + 2] << 8) | _d[_base + o + 3]);

    /// <summary>Each entry: (tag, type, count, offset of its value field or data).</summary>
    void ReadIfd(int at, int limit, Action<int, int, int, int> entry)
    {
        if (at <= 0 || at + 2 > limit) return;
        int n = U16(at);
        for (int i = 0; i < n && i < 512; i++)
        {
            int o = at + 2 + i * 12;
            if (o + 12 > limit) return;
            int tag = U16(o), type = U16(o + 2);
            long count = U32(o + 4);
            int size = count * (type == 3 ? 2 : type == 4 || type == 9 ? 4 : type == 5 || type == 10 ? 8 : 1) > 4 ? 1 : 0;
            int valueAt = size == 1 ? (int)U32(o + 8) : o + 8;
            if (valueAt < 0 || valueAt >= limit) continue;
            Tags++;
            entry(tag, type, (int)Math.Min(count, 4096), valueAt);
        }
    }

    // the value field already points at the data for anything over 4 bytes
    int Ptr(int off, int type, int count) => off;

    long Value(int type, int off) => type == 3 ? U16(off) : type == 4 || type == 9 ? U32(off) : _d[_base + off];

    double Rational(int off)
    {
        long num = U32(off), den = U32(off + 4);
        return den == 0 ? 0 : (double)num / den;
    }

    double Degrees(int off) => Rational(off) + Rational(off + 8) / 60 + Rational(off + 16) / 3600;

    string Ascii(int off, int count)
    {
        var sb = new StringBuilder();
        for (int i = 0; i < count && _base + off + i < _d.Length; i++)
        {
            byte b = _d[_base + off + i];
            if (b == 0) break;
            sb.Append(b >= 32 && b < 127 ? (char)b : ' ');
        }
        return sb.ToString().Trim();
    }

    /// <summary>"2024:05:01 10:22:33" -> "1-5-2024 10.22".</summary>
    public string When()
    {
        var t = DateTime;
        if (t.Length < 16 || t[4] != ':') return t;
        return Num(t.Substring(8, 2)) + "-" + Num(t.Substring(5, 2)) + "-" + t.Substring(0, 4) + " " + t.Substring(11, 2) + "." + t.Substring(14, 2);
    }

    static string Num(string two) => two.Length == 2 && two[0] == '0' ? two.Substring(1) : two;

    static string Dec(double v, int places)
    {
        long scale = places == 1 ? 10 : places == 2 ? 100 : 10000;
        long r = (long)Math.Round(Math.Abs(v) * scale);
        string frac = (r % scale).ToString();
        while (frac.Length < places) frac = "0" + frac;
        return (v < 0 ? "-" : "") + (r / scale) + (places > 0 ? "," + frac : "");
    }

    /// <summary>A one-line summary for the caption.</summary>
    public string Summary()
    {
        var parts = new List<string>();
        string cam = Model.StartsWith(Make) || Make.Length == 0 ? Model : Make + " " + Model;
        if (cam.Trim().Length > 0) parts.Add(cam.Trim());
        if (DateTime.Length > 0) parts.Add(When());
        var expo = new List<string>();
        if (FNumber > 0) expo.Add("f/" + Dec(FNumber, 1));
        if (ExposureSeconds > 0) expo.Add(ExposureSeconds < 1 ? "1/" + (int)Math.Round(1 / ExposureSeconds) + " s" : Dec(ExposureSeconds, 1) + " s");
        if (Iso > 0) expo.Add("ISO " + Iso);
        if (FocalMm > 0) expo.Add((int)Math.Round(FocalMm) + " mm");
        if (expo.Count > 0) parts.Add(string.Join(" ", expo));
        if (HasGps) parts.Add(Dec(Latitude, 4) + "; " + Dec(Longitude, 4));
        return string.Join("  -  ", parts);
    }

    // ---- writing (tests: a photo with known EXIF) -------------------------------

    /// <summary>A minimal big-endian EXIF block (TIFF, IFD0 + Exif IFD + GPS IFD).</summary>
    public static byte[] Sample(string make, string model, int orientation, string date, int expDen, int fNum10, int iso, double lat, double lon)
    {
        var o = new List<byte>();
        void U16(int v) { o.Add((byte)(v >> 8)); o.Add((byte)v); }
        void U32(long v) { o.Add((byte)(v >> 24)); o.Add((byte)(v >> 16)); o.Add((byte)(v >> 8)); o.Add((byte)v); }
        var data = new List<byte>();
        // layout: header 8 | IFD0 (6 entries) | Exif IFD (4) | GPS IFD (4) | data
        int ifd0 = 8, ifd0Len = 2 + 6 * 12 + 4, exif = ifd0 + ifd0Len, exifLen = 2 + 4 * 12 + 4, gps = exif + exifLen, gpsLen = 2 + 4 * 12 + 4;
        int dataAt = gps + gpsLen;
        int Add(byte[] b) { int at = dataAt + data.Count; data.AddRange(b); if ((data.Count & 1) == 1) data.Add(0); return at; }
        byte[] Str(string s) { var b = new byte[s.Length + 1]; for (int i = 0; i < s.Length; i++) b[i] = (byte)s[i]; return b; }
        byte[] Rats(params long[] v) { var b = new byte[v.Length * 4]; for (int i = 0; i < v.Length; i++) { b[i * 4] = (byte)(v[i] >> 24); b[i * 4 + 1] = (byte)(v[i] >> 16); b[i * 4 + 2] = (byte)(v[i] >> 8); b[i * 4 + 3] = (byte)v[i]; } return b; }
        long Deg(double x) => (long)Math.Floor(Math.Abs(x));
        long Min(double x) => (long)Math.Floor((Math.Abs(x) - Deg(x)) * 60);
        long Sec100(double x) => (long)Math.Round(((Math.Abs(x) - Deg(x)) * 60 - Min(x)) * 60 * 100);
        int pMake = Add(Str(make)), pModel = Add(Str(model)), pDate = Add(Str(date));
        int pExp = Add(Rats(1, expDen)), pF = Add(Rats(fNum10, 10));
        int pLat = Add(Rats(Deg(lat), 1, Min(lat), 1, Sec100(lat), 100)), pLon = Add(Rats(Deg(lon), 1, Min(lon), 1, Sec100(lon), 100));
        o.Add((byte)'M'); o.Add((byte)'M'); U16(42); U32(ifd0);
        void Entry(int tag, int type, long count, long value, bool shortInline = false)
        {
            U16(tag); U16(type); U32(count);
            if (shortInline) { U16((int)value); U16(0); } else U32(value);
        }
        U16(6);
        Entry(0x010F, 2, make.Length + 1, pMake);
        Entry(0x0110, 2, model.Length + 1, pModel);
        Entry(0x0112, 3, 1, orientation, shortInline: true);
        Entry(0x0132, 2, date.Length + 1, pDate);
        Entry(0x8769, 4, 1, exif);
        Entry(0x8825, 4, 1, gps);
        U32(0);
        U16(4);
        Entry(0x829A, 5, 1, pExp);
        Entry(0x829D, 5, 1, pF);
        Entry(0x8827, 3, 1, iso, shortInline: true);
        Entry(0x9003, 2, date.Length + 1, pDate);
        U32(0);
        U16(4);
        U16(1); U16(2); U32(2); o.Add((byte)(lat < 0 ? 'S' : 'N')); o.Add(0); o.Add(0); o.Add(0);
        Entry(2, 5, 3, pLat);
        U16(3); U16(2); U32(2); o.Add((byte)(lon < 0 ? 'W' : 'E')); o.Add(0); o.Add(0); o.Add(0);
        Entry(4, 5, 3, pLon);
        U32(0);
        o.AddRange(data);
        return o.ToArray();
    }

    /// <summary>The block wrapped as a JPEG APP1 segment (SOI + APP1 + EOI: enough for the parser).</summary>
    public static byte[] SampleJpeg(byte[] tiff)
    {
        var o = new List<byte> { 0xFF, 0xD8, 0xFF, 0xE1 };
        int len = tiff.Length + 8;
        o.Add((byte)(len >> 8)); o.Add((byte)len);
        foreach (char ch in "Exif") o.Add((byte)ch);
        o.Add(0); o.Add(0);
        o.AddRange(tiff);
        o.Add(0xFF); o.Add(0xD9);
        return o.ToArray();
    }
}

# Console System App (C#)

Template app Buitenzorg varian **console** (requirements.md §11.1, §13.2).

## Menjalankan (host simulation)

```
dotnet run
dotnet run -- --ticks
```

Saat runtime managed Buitenzorg sudah berjalan di bare metal (roadmap v0.4
"Tunas"), app yang sama berjalan tanpa perubahan di atas kernel — `BzSys`
otomatis memilih backend syscall native.

## File

- `app.manifest` — manifest terpadu (id, type, language, permissions, theme)
- `Program.cs` — stdin/stdout + akses syscall API

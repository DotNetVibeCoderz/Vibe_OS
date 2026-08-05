# System Services (v0.5 "Dahan")

Milestone v0.5: **"service C# jalan sebagai proses; async I/O yang
benchmark-able"** (requirements.md §16). Semuanya berjalan di kernel dan
diverifikasi tiap boot lewat marker `MILESTONE: …` yang juga dicek smoke test.

[English](system-services.md) · **Bahasa Indonesia** · ← [Indeks dokumentasi](README.id.md)

## VFS + FAT read/write (`vfs.rs`, `fat.rs`, `ramdisk.rs`)

- **Mount table**: `vfs::mount(name, device, read_only)`. Path berbentuk
  `/<mount>/<FILE>`. Saat boot: `/disk` (boot disk, read-only) dan `/ram`
  (ramdisk, read-write).
- **FAT write** (`fat::write_file`, FAT12): alokasi rantai cluster bebas, tulis
  data, link FAT (dua salinan), dan buat/timpa entri direktori root 8.3.
- **RAM disk** (`ramdisk.rs`) di-format in-kernel (`fat::format_fat12`) lalu
  di-mount `/ram`. Demo menulis `/ram/DAHAN.TXT` dan membacanya kembali (verified).

## Service / init manager (`service.rs`)

`register(name, deps, entry)`, lalu `start_all()`: startup **paralel &
dependency-aware** di atas scheduler — tiap service jadi task, hanya dijalankan
setelah semua dependensinya minimal `Running`. Demo: `logger → {netd, storaged} →
app`, dengan urutan startup diverifikasi.

> Catatan implementasi: `spin::Mutex` tidak reentrant. `start_all` mengambil
> *snapshot* state di satu lock, lalu memutuskan kesiapan tanpa mengunci ulang
> (menghindari deadlock lock-di-dalam-lock).

## Async I/O, io_uring-style (`aio.rs`)

Submission queue (SQ) + completion queue (CQ). Submitter push SQE (`Nop`,
`ReadBlock`); task worker menguras SQ, melakukan I/O ke block device, lalu push
CQE dengan `user_data` yang sama. `benchmark(count)` mengukur ops per tick timer
(PIT ~18,2 Hz) → **ops/detik**. "Benchmark-able" sesuai milestone.

## Networking awal (`net.rs`)

Stack minimal **Ethernet + ARP + IPv4 + ICMP** di atas trait `NetDevice`, di-drive
lewat **loopback**. Demo: kirim ICMP echo request ke IP sendiri → stack
memprosesnya (ARP self, IP, ICMP) → membuat echo reply → round-trip terverifikasi
(counter reply naik). Trait yang sama nanti menopang driver e1000 (hardware NIC —
roadmap lanjutan).

## C# service sebagai proses (ring 3)

Init manager meluncurkan `SVC.ELF` (program C# kedua,
`userland/hello-csharp/svc.cs`) sebagai proses ring-3 via `run_user_elf` (load ELF
→ `enter_user` → unmap saat keluar). Program mencetak lewat syscall dan keluar
bersih.

> **SSE wajib**: kode hasil NativeAOT memakai register xmm (mis. `xorps` di
> `Console.WriteLine(int)`). Kernel meng-enable SSE/SSE2 (`gdt::enable_sse`:
> CR0.EM=0, CR0.MP=1, CR4.OSFXSR+OSXMMEXCPT) sebelum menjalankan kode managed —
> tanpa ini instruksi xmm memicu #UD → double fault.

## Menjalankan ulang program user

`run_user_elf` hanya boleh dipanggil saat **satu-satunya task runnable adalah boot
task** (agar preemption inert saat di ring 3, sesuai model v0.4). Karena itu
`dahan_demo` men-drain task yang masih menyelesaikan (`yield_now`) sebelum
meluncurkan C# service. Alamat user tetap (`0x400000`, stack `0x7000_0000`), jadi
program sebelumnya harus di-unmap dulu — `run_user_elf` melakukannya.

---

← [Indeks dokumentasi](README.id.md) · *Buitenzorg OS — dibuat oleh Gravicode Studios, dipimpin oleh Kang Fadhil.*

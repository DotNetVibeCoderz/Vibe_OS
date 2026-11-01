# Official Buitenzorg OS wallpapers

AI-generated artwork made for Buitenzorg OS (generated with Nano Banana 2 via
the Rodin MCP server). Kept here as baseline JPEGs: `kernel/bzimage/build.rs`
decodes them, downscales to 800×450 and puts them on `/disk` as 24-bit BMPs
(the kernel reads BMP only). In the OS: `bg list`, `bg <name>`.

| File | `bg` name | Theme |
|---|---|---|
| `nature.jpg` | `nature` (default) | Bogor Botanical Gardens at dawn |
| `futuristic.jpg` | `futuristic` | Neon garden city |
| `city.jpg` | `city` | Rainy city at blue hour |
| `art.jpg` | `art` | Modern kawung/parang batik |
| `surreal.jpg` | `surreal` | Upside-down banyan over the sea |
| `imaginary.jpg` | `imaginary` | Floating rainforest islands |
| `space.jpg` | `space` | Ringed planet + nebula |
| `minimal.jpg` | `minimal` | Twilight mountains |

Adding one: drop a **baseline** (not progressive) JPEG here, add it to
`OFFICIAL_WALLPAPERS` in `kernel/bzimage/build.rs` and to `OFFICIAL` in
`kernel/bzkernel/src/wallpaper.rs` (8.3 file name on `/disk`).

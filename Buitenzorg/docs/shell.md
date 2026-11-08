# The shell and shell scripts

The desktop **Terminal** runs Buitenzorg's built-in shell (`bzsh`). It reads
like a small POSIX shell: pipes, redirection, variables, scripts and aliases,
over the OS's own commands plus a set of coreutils.

## Editing a command line

| Key | Does |
|---|---|
| ← / → | move the caret |
| Home / End, Ctrl+A / Ctrl+E | start / end of the line |
| Delete, Backspace | delete at / before the caret |
| Ctrl+U / Ctrl+K / Ctrl+W | delete to the start / to the end / the word before |
| ↑ / ↓ | command history (kept in `/disk/HISTORY.TXT`, 200 lines) |
| Tab | complete a command or a file name |
| Ctrl+L / Ctrl+C | clear the screen / drop the line |
| PageUp / PageDown, mouse wheel | scroll back |

## Command lines

```sh
ls /disk | grep -i elf | sort -r | head -n 3     # pipes
echo halo > /ram/A.TXT; cat < /ram/A.TXT >> /ram/B.TXT   # redirection
mkdir /ram/X && cp /ram/A.TXT /ram/X || echo gagal       # ; && ||
X=kebun; echo "nama: $X, status $?"                      # variables, $?
ls /disk/H*.ELF                                          # globs * ?
```

Quotes: `'...'` is literal, `"..."` expands `$NAME`, `${NAME}`, `$?`; `\`
escapes one character. `PATH` (default `/disk:/ram`), `PWD`, `HOME`, `USER` and
`SHELL` are built in; `env`/`set`/`export NAME=value`, `unset`.

**Coreutils:** `cat grep head tail wc sort uniq tee cp mv mkdir rmdir touch
find hexdump/xxd seq basename dirname true false env set export unset history
test [`. Everything else (`ls`, `rm`, `ps`, `kill`, `run`, `bz`, `ipconfig`,
`ping`, `theme`, ...) is listed by `help`.

## Tabs and jobs

**Tabs.** One Terminal window holds up to 8 sessions. Each has its own
scrollback, input line, working directory and variables; the command history
is shared. With more than one tab the first row is the tab bar
(`[1:/disk]  2:/ram*` -- `*` = jobs running).

| Key / command | |
|---|---|
| Ctrl+T, `tab new` | a new tab |
| Ctrl+N / Ctrl+P, `tab N` | next / previous tab, tab N |
| Ctrl+D on an empty line, `tab close` | close the tab (its running programs go on) |
| `tab` | list the tabs |

**Jobs.** A program started from a tab (`run hello`, or just `hello`) is a
*job* of that tab, and what it prints appears in the tab.

| | |
|---|---|
| `run X` | foreground: the prompt waits until X ends; **Ctrl+C** ends it, **Ctrl+Z** lets it go on in the background |
| `run X &` | background: the prompt comes back at once (`[1] 1004`); the tab says `[1]  Selesai (kode 0)` when it ends |
| `jobs` | the tab's jobs, running or finished, with their exit codes |
| `fg [%N]` | wait for a running job again (the newest by default) |
| `kill %N` | end job N |

A window app started in the foreground keeps the prompt waiting until its
window closes -- add `&`, or press Ctrl+Z. Limits: there is no stdin (a
program reads the keyboard only through its own window), so `fg` only waits;
jobs belong to the Terminal (`jobs` from `bz run` says so); a closed tab's
programs keep running, unlisted. The self-test boot drives all of it with
keystrokes (`MILESTONE: TERMINAL TABS OK`).

## Aliases and `which`

```sh
alias lsd='ls /disk'      # define
lsd | grep -c ELF         # the first word of each pipeline stage is replaced
alias                     # list;  unalias lsd  /  unalias -a
which lsd cat ls hello    # alias, built-in, script, package or program
```

## Scripts

A script is a text file ending in `.SH` (the volumes are FAT, 8.3 names). Run
it with `sh FILE [args]`, `source FILE` / `. FILE`, or just by name: a path
(`/disk/BUILD.SH`, `./BUILD.SH`) or `build` when `BUILD.SH` is in a `$PATH`
folder.

```sh
# /disk/SALAM.SH -- greet every argument
if [ $# -eq 0 ]; then
    echo "pakai: salam NAMA..."
    exit 1
fi
for nama in $@; do
    echo "Halo, $nama!"
done
touch /ram/BUSY
while test -f /ram/BUSY; do
    echo "bekerja..."
    rm /ram/BUSY
done
```

* `$0`..`$9`, `$#`, `$@` (one word per argument, also inside `"$@"`).
* `if CMD; then ...; elif CMD; then ...; else ...; fi`,
  `for X in WORDS; do ...; done` (no `in`: the arguments),
  `while CMD; do ...; done`, `exit [N]`, `#` comments; statements on one line
  are separated by `;`.
* Conditions are commands; `test` / `[ ]` knows `-e -f -d PATH`, `-z -n STR`,
  `A = B`, `A != B`, `N -eq -ne -lt -le -gt -ge M`, `!`, `-a`, `-o`.
* **Not yet:** `$(...)` command substitution and `$((...))` arithmetic, functions,
  `case`, here-documents, background jobs (`&`).

**Limits:** scripts run on the desktop's kernel task, so one run executes at
most 20,000 statements and a `while` loop at most 5,000 rounds; a runaway
script stops with a message instead of freezing the desktop. Scripts may call
scripts 8 levels deep.

## The startup profile

`/disk/PROFILE.SH` runs every time the Terminal starts — the place for your
aliases and variables:

```sh
alias ll='ls /disk'
alias h='history'
EDITOR=editor
```

Implementation: `kernel/bzkernel/src/shell.rs` (parsing, pipelines),
`bzsh.rs` (aliases, scripts, `which`), `coreutils.rs`, `terminal.rs` (line
editing, history, completion).

---

← [Documentation index](README.md) · *Buitenzorg OS — made by Gravicode Studios, led by Kang Fadhil.*

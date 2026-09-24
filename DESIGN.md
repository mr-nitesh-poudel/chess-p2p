# Design notes

The long version of what [the README](README.md) skims over: how the program
is put together, how the pieces are drawn, and how two players find each
other. Written for whoever works on this next, including me in six months.

## Layout

- **`session/`** — pairing, with nothing game-specific in it. [`iroh`] holds a
  QUIC connection between the two players, hole-punching a direct link where
  it can and falling back to a relay where it can't. Which game is played is
  agreed in the handshake. See [Pairing](#pairing).
- **`net.rs`** — a game's messages over a paired session. One bi-directional
  stream carries newline-delimited text; what the lines say is up to the game,
  and the only word reserved here is `bye`.
- **`games/`** — the games, and the table each is played at.
  - `Play` is what a game implements: its own keys, clicks and drawing, and
    the opponent's lines. It says whether it used a key; the ones it didn't
    fall through to the table.
  - `Table` is the shell every game sits in. It owns the connection (`Ctx`),
    and the things every game shares: `q`/`esc` to leave, asking first if the
    game is still in play; `m` for the mouse; `c` to copy the share code;
    Ctrl-C, which always quits and never reaches a game. The main loop holds
    a `Box<Table<dyn Play>>` and never learns which game is on it.
  - `chat.rs` is the players talking, and it is the table's too: a line
    starting `chat ` is taken before the game sees it, cleaned of control
    characters, and kept. While the player is typing, keys go to the chat and
    not the game. Where the panel sits is the game's call: it draws it with
    `chat::draw` and reports its place through `Play::chat_area`, so clicks
    land on it. Chess puts it right of the board, or under the moves when the
    board would otherwise have to shrink. Older builds ignore `chat` lines,
    so it needed no protocol bump.
  - `chrome.rs` draws what every game's screen shares: the connection status
    and the footer, with the table's own questions in it.
  - `Kind` is the registry: each game supplies a `Descriptor` (name, wire id,
    how to start one) and takes one line in `Kind::ALL`.
  - **`games/chess/`** — `rules.rs` holds the position and legality, with
    [`shakmaty`] supplying move generation, so castling, en passant, promotion
    and mate detection are handled properly. `app.rs` turns keys and messages
    into state changes. `ui/` draws with [`ratatui`], reading state and never
    writing it: `board.rs` for the squares and what stands on them, `pieces.rs`
    for how a piece is drawn at a given size, `panels.rs` for everything
    around the board, `bar.rs` for the evaluation bar. `canvas.rs` holds the
    silhouettes, and `protocol.rs` is what the two sides say: `move e2e4`,
    `resign`, `draw`. `engine.rs` runs an engine and `analysis.rs` is what is
    made of it; see [Analysis](#analysis).
  - `chat.rs` is the players talking, and it is the table's: a line starting
    `chat ` is taken before the game sees it, and cleaned of control
    characters. Where the panel sits is the game's call, reported through
    `Play::chat_area` so clicks land on it.
- **`lobby/`** — the first screen: the game, the menu, the code box with its
  completion, and friends.
- **`hub.rs`** — the main loop. It pairs players, answers invites, and hands
  each game its input without knowing which game it is.
- **`profile.rs`** — your identity, name and friends, kept between runs.
- **`clipboard.rs`** — OSC 52, plus the platform's clipboard tool.
- **`ui.rs`** — the palette and the few drawing helpers every screen uses.

Adding a game means a module under `games/` with a type that implements
`Play`, a `Descriptor`, and a line in `Kind::ALL`. The lobby, the command
line, pairing, friends and invites pick it up from there. `games/mod.rs`
spells out what a game can rely on from the table and what it must do in
return; `tests/table.rs` runs a toy game through the table to prove none of
it depends on chess.

A game's `Geometry` decides what is drawn *and* what a click hits, so the two
cannot drift apart.

The crate forbids `unsafe`, and release builds keep overflow checks on: a
wrapped screen coordinate would quietly draw nonsense rather than fail, and
nothing here is hot enough to notice the checks.

There is no referee. Both peers run the same rules over their own copy of the
game, and nothing the peer sends is taken on trust:

- **Lines are bounded.** A peer's line is read at most 4 KiB at a time
  (`session/lines.rs`). Without that, anyone who can dial us — in the lobby,
  anyone who knows our endpoint id, before proving anything — could send one
  endless line and exhaust our memory. The reader is also cancel safe: it
  races our own sends in `select!`, and a read given up on keeps the part of
  a line it had rather than dropping it, which would split the next message
  in two. The engine's output goes through the same reader.
- **Names are cleaned where they arrive** (`session/name.rs`): no control
  characters, 24 characters at most.
- **A game checks every message against its own state.** In chess, a move is
  refused unless it is the peer's turn — the rules alone would happily play a
  legal move for *our* side — and a finished game takes no more messages, so
  a late `resign` cannot overturn a checkmate. Messages are matched exactly:
  `resign please` is not a resignation.

## Analysis

`a` runs an engine over the position on the screen: Stockfish, or anything
that speaks UCI, found on the `PATH`, where Homebrew and Debian put it, or at
`TUITUI_ENGINE`. It is the player's own program, run as a child process and
never built in, which keeps its GPL out of this crate.

- **One task owns the process** (`engine.rs`). The game sends it the
  position on the screen, searched first and deeply, and the whole game,
  searched quickly after, to grade the moves. Results go into a table keyed
  by FEN that the drawing reads, and the task wakes the main loop through
  `Ctx::waker` rather than having it poll. Scores come back for the side to
  move and are turned round to be white's.
- **Nothing the engine prints is trusted.** Unparseable lines are skipped,
  scores are held in range so no sum on them can overflow, lines are bounded,
  writes to it time out, and a best move is only shown if it is legal. An
  engine that dies turns into a message, not a crash. When analysis is
  switched off, or the game is left, the engine is told to quit, then killed
  and waited for, so nothing is left running.
- **Not while a game against someone is on.** An engine's opinion is advice,
  so the key does nothing in a network game until it is decided; hot-seat,
  with both players at the keyboard, may analyse whenever. This is a
  courtesy, not a guarantee: with no referee, nothing stops a modified
  client, or a second window, from consulting an engine.
- **Grades** (`analysis.rs`) are by how much of the mover's chances a move
  gave away, on the curve Lichess fitted to real games: 10% is an
  inaccuracy, 20% a mistake, 30% a blunder. Positions the game is over in are
  scored by the rules, not the engine.
- **Looking back** needs no engine. `Game` keeps every position it has left;
  `,` and `.` step through them, the arrows too once the game is over, and
  the board draws the position still, with no cursor, marks or animation.

## Pieces

The board sizes itself to your terminal, from 3x1 squares up to 11x5, and
draws the best pieces the square can carry.

### Canvas, 9x4 and up

The pieces are drawn with ratatui's `Canvas` widget, on a grid of two dots
across and four down to each cell: 22x20 dots on the biggest board. Terminal
cells are about twice as tall as they are wide, which makes the dots nearly
square and lets the pieces keep their proportions. The pieces are vector
silhouettes (circles, ellipses and polygons in a unit box, in `canvas.rs`)
sampled at each dot, so the same drawing serves every square size, and a
moving piece slides a dot at a time rather than a cell at a time. Where it
passes over another piece, the two sets of dots are merged rather than one
hiding the other.

By default the dots are octants, which fill each dot in solid so a piece reads
as one shape. Octants are new to Unicode (16.0), and a terminal that cannot
draw them shows `�`; press `p` once for braille, the same dots drawn round
with gaps between them, which every terminal can show. Ghostty, kitty, WezTerm
and foot draw octants themselves, whatever the font.

Every dot in a cell shares one colour, so each piece is one solid colour:
white for white, near-black for black. A canvas also paints its whole area's
background, which would wipe out the squares, so the pieces are drawn into a
scratch buffer and only the cells with dots in them are copied onto the board.

### Sprites, 7x3 and up

Below 9x4 there are too few dots to tell the pieces apart, so smaller squares
use sprites. Every cell is drawn as `▀`, whose foreground paints the top half
and whose background paints the bottom, so each character holds two stacked
pixels — a 9x4 square becomes a 9x8 bitmap. This needs nothing but truecolor,
so it works in every terminal, with no image protocol to negotiate and no
assets to ship. Both sides are outlined in near-black, with black's body
lifted off its outline far enough that the outline still reads against it.

Sprites are drawn a size below their square (5x5, 7x7 and 9x8) and sunk
towards the bottom, so a margin of board shows around each piece and pieces
stand on their squares rather than filling them.

Half blocks stack two pixels in a cell and a cell carries two colours, so
every sprite pixel lands exactly as drawn. Quadrants or sextants would divide
the cell further, but an outline, a body and the square showing through are
three colours and a cell can only hold two, so anywhere an edge runs
diagonally the renderer would have to approximate. Real detail past this point
means a terminal graphics protocol.

There is also a big-letter style on the same pipeline: the piece's initial as
a 5x5 bitmap with a one-pixel border grown around it at draw time, which comes
out 7x7 — the same room the mid sprite takes, so the two can be compared side
by side. The border closes up the counters of a letter that small, so the
stroke has to carry the contrast against the border rather than against the
square: black's stroke is lifted to a mid grey for that, while keeping the
same near-black border the sprites use.

### Smaller still

Three-row character art, then figurines, then letters. `p` cycles through them
by hand: pick letters if your terminal renders the chess glyphs as
double-width and the fallbacks look misaligned. Only sprites animate; the
character styles just arrive.

### Marks and movement

A move slides its piece across the board over 160ms, eased in and out. The
travelling piece is drawn straight onto the buffer after the board, so it is
not tied to the square grid and can sit halfway between two of them.

Squares you can move to are marked with a dot, a shade darker than the square,
and a capture has its corners filled in round a circle instead, since a ring
would run through the piece — pieces fill their square top to bottom, and the
corners are the only part always free. With octant pieces the marks are drawn
in octants and come out round; otherwise they fall back to half blocks, which
every terminal draws.

Checkmate plays in three acts once the mating piece lands: the king shudders
while its square flashes red, then topples away from whatever mated it as the
board goes dark, and then the verdict is spelled out in block letters across
the half of the board the king is not on. Resigning is quieter — a moment's
stillness, then the king lays itself down. A check pulses the king's square
red a few times and then holds a steady glow until it is answered.

### The mouse

While the mouse is captured, the terminal's own text selection is disabled, so
you cannot drag-select text. If copying your share code with `c` did not work,
press `m` to hand the mouse back (or hold shift, in most terminals), select
it, and press `m` again.

The copy asks the terminal to set the clipboard (OSC 52), which is what works
over SSH, and on a local machine also runs `pbcopy`, `wl-copy`, `xclip`,
`xsel` or `clip.exe`, whichever is there. Some terminals ignore OSC 52 (macOS
Terminal, and tmux unless `set -g set-clipboard on`), so over SSH in one of
those, select the code by hand.

## Pairing

A code is a number and three words from the BIP39 English list, which comes to
about 40 bits: short enough to read out, too many to guess.

1. **Rendezvous.** Both sides stretch the code with Argon2id (64 MiB) into the
   same ed25519 keypair. The host signs a [pkarr] record with it, naming its
   iroh endpoint, and publishes that to the Mainline DHT, BitTorrent's network
   of millions of nodes. The joiner derives the same public key and looks the
   record up. Nobody runs a server for this. Publishing and looking up each
   take a few seconds; a joiner who arrives before the record has spread keeps
   asking for a minute. The stretch is what keeps the codes short: finding
   live games by walking the code space would mean doing it for every possible
   code.
2. **Handshake.** Once connected, the two sides run SPAKE2 with the code and
   confirm the key they reached, bound to both endpoint ids. Someone without
   the code gets one guess per connection, and the host stops listening after
   three wrong ones.
3. **Game.** The host names the game and version it is playing (`game chess 1`)
   and the joiner accepts it or backs out, so every game shares one pairing
   protocol.

A code stops working an hour after it is published, or as soon as the host
leaves the game it was for.

### Friends

Every player keeps one secret key, so their endpoint id stays the same from
run to run. Pairing by code is how two players first learn each other's ids;
from then on each can dial the other by id alone, and iroh's discovery finds
the rest. iroh authenticates both ends of every connection, so an invite needs
no code: the host sees who is really dialling, and only asks its player about
friends it already has. Anyone else, or anyone who calls while a game is on,
is turned away with a reason the other side can show (`busy`, `unknown`,
`declined`).

One endpoint serves the whole run, and one listener answers everything that
dials it. What it does depends on where the player is: in the lobby it passes
friends' invites on, while hosting it pairs by code, and during a game it
turns everyone away.

### The profile

It lives in `tui-tui` under your config directory — the crate's name, not the
command's — (`~/Library/Application Support` on macOS, `~/.config` on Linux),
or in `$TUI_TUI_HOME` if set:

- `identity.key` — the secret key, readable only by you
- `profile.json` — your name and friends
- `identity.lock` — held while running

A second copy started while the first holds the lock runs as a guest, with a
throwaway key and nothing saved, rather than answering to the same id. To run
two players on one machine, give the second its own `TUI_TUI_HOME`.

## Tests

`cargo test` covers the rules (castling's rook-square quirk, promotion, en
passant, mate), checks that every square hit-tests correctly at four terminal
sizes in both orientations along with the click and drag gestures, reads the
sprites back out of a rendered buffer to confirm all six pieces are drawn and
are told apart from each other, and stands up two real iroh endpoints in one
process to play moves between them. The pairing tests check that codes parse
the way people retype them, that a wrong code is refused and the host gives up
after three, and that a joiner backs out of a game it does not have. The lobby
tests cover the menu, the game row, typing and pasting codes, Tab completion,
flagging bad words as they are typed, clicking items, friends, renaming and
answering invites. The invite tests cover accepting, refusing, a busy player,
an unknown game and a withdrawn invite; the profile tests cover the identity
surviving a restart, the lock, the key's permissions, and saving friends. The
games tests cover the registry, the seats and chess's messages, and one test
draws and clicks every screen at every terminal size from nothing up to a
maximised window, since a screen too small to hold a board is where the
arithmetic gives out.

`cargo test -- --ignored` also runs pairing over the real DHT, publishing a
code and looking it up. It needs the internet and takes around ten seconds.

`cargo run --example render` prints the UI at several sizes as plain text,
which is the quickest way to iterate on layout.

[`shakmaty`]: https://docs.rs/shakmaty
[`iroh`]: https://docs.rs/iroh
[pkarr]: https://pkarr.org
[`ratatui`]: https://docs.rs/ratatui

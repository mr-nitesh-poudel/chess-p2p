# tui-tui

Terminal chess for two people, played over a direct peer-to-peer connection.
No server to run, no account, no port forwarding — one of you reads out a
short code, the other types it in.

```
cargo run --release                                 # the lobby
cargo run --release -- host                         # or skip it: host a game,
cargo run --release -- join 42-tiger-marble-ocean   # join one,
cargo run --release -- local                        # or share a keyboard
```

Anyone you have played turns up in the lobby under **friends**. Pick one to
challenge them directly, with no code: their lobby asks them to accept.
Leaving a game (`q`) brings you back to the lobby with your last opponent
already selected, so a rematch is one keypress away. `x` forgets a friend, and
**Your name** is what your friends see.

The lobby lets you host, join or share a keyboard. Hosting puts your code
on the clipboard straight away; `c` copies it again. To join, type the code
or paste it. Pasting the whole `tui-tui join ...` command works too. You can
also start typing the number from the menu. Tab finishes a word once only
one word fits, and a word that is not in the list is flagged as you type.

Codes are forgiving to retype: any case, spaces instead of dashes, and each
word can be cut down to its first four letters (`42 tige marb ocea`).

The copy asks the terminal to set the clipboard (OSC 52), which is what works
over SSH, and on a local machine also runs `pbcopy`, `wl-copy`, `xclip`,
`xsel` or `clip.exe`, whichever is there. Some terminals ignore OSC 52 (macOS
Terminal, and tmux unless `set -g set-clipboard on`), so over SSH in one of
those, select the code by hand.

The host plays white, the joiner plays black.

## Controls

Click a piece and click where it goes, or drag it there — whichever you prefer.
Right-click puts a piece back down. Everything works from the keyboard too:

| key | |
|---|---|
| arrows / `hjkl` | move the cursor |
| `enter` / `space` | pick a piece up, or put it down |
| `f` | flip the board |
| `p` | cycle piece style: octants, braille, sprites, big letters, art, figurines, letters |
| `m` | turn mouse reporting off (see below) |
| `c` | copy your share code, while you wait for an opponent |
| `r` | resign (confirm with `y`) |
| `d` | offer or accept a draw |
| `q` / `esc` | leave the game, back to the lobby |
| `ctrl-c` | quit |

Promotion opens a prompt: click a piece, or `←`/`→` then `enter`, or press
`q` `r` `b` `n`.

## Pieces

The board sizes itself to your terminal, from 3x1 squares up to 11x5, and draws
the best pieces the square can carry.

On squares of 9x4 and up the pieces are drawn with ratatui's `Canvas` widget,
on a grid of two dots across and four down to each cell: 22x20 dots on the
biggest board. Terminal cells are about twice as tall as they are wide, which
makes the dots nearly square and lets the pieces keep their proportions. The
pieces are vector silhouettes (circles, ellipses and polygons in a unit box,
in `canvas.rs`) sampled at each dot, so the same drawing serves every square
size, and a moving piece slides a dot at a time rather than a cell at a time.
Where it passes over another piece, the two sets of dots are merged rather
than one hiding the other.

By default the dots are octants, which fill each dot in solid so a piece reads
as one shape. Octants are new to Unicode (16.0), and a terminal that cannot
draw them shows `�`; press `p` once for braille, the same dots drawn round with
gaps between them, which every terminal can show. Ghostty, kitty, WezTerm and
foot draw octants themselves, whatever the font.

Every dot in a cell shares one colour, so each piece is one solid
colour: white for white, near-black for black. A canvas also paints its whole area's background, which would wipe out the
squares, so the pieces are drawn into a scratch buffer and only the cells with
dots in them are copied onto the board.

Below 9x4 there are too few dots to tell the pieces apart, so smaller squares
use sprites instead.

At 7x3 and above the other styles are proper sprites. Every cell is drawn as `▀`, whose
foreground paints the top half and whose background paints the bottom, so each
character holds two stacked pixels — a 9x4 square becomes a 9x8 bitmap. This
needs nothing but truecolor, so it works in every terminal, with no image
protocol to negotiate and no assets to ship. Both sides are outlined in
near-black, with black's body lifted off its outline far enough that the outline
still reads against it.

Sprites are drawn a size below their square (5x5, 7x7 and 9x8) and sunk towards
the bottom, so a margin of board shows around each piece and pieces stand on
their squares rather than filling them.

Half blocks stack two pixels in a cell and a cell carries two colours, so every
sprite pixel lands exactly as drawn. Quadrants or sextants would divide the cell
further, but an outline, a body and the square showing through are three colours
and a cell can only hold two, so anywhere an edge runs diagonally the renderer
would have to approximate. Real detail past this point means a terminal graphics
protocol.

There is also a big-letter style on the same pipeline: the piece's initial as a
5x5 bitmap with a one-pixel border grown around it at draw time, which comes out
7x7 — the same room the mid sprite takes, so the two can be compared side by
side. The border closes up the counters of a letter that small, so the stroke
has to carry the contrast against the border rather than against the square:
black's stroke is lifted to a mid grey for that, while keeping the same
near-black border the sprites use.

Smaller squares fall back to three-row character art, then to figurines, then to
letters. `p` cycles through them by hand: pick letters if your terminal renders
the chess glyphs as double-width and the fallbacks look misaligned. Only sprites
animate; the character styles just arrive.

A move slides its piece across the board over 160ms, eased in and out. The
travelling piece is drawn straight onto the buffer after the board, so it is not
tied to the square grid and can sit halfway between two of them.

Squares you can move to are washed green rather than marked with a dot, more
strongly for a capture than a quiet move. The wash is mixed against the square's
own colour, so the board still reads underneath and a washed square under the
cursor shows both.

While the mouse is captured, the terminal's own text selection is disabled, so
you cannot drag-select text. If copying your share code with `c` did not
work, press `m` to hand the mouse back (or hold shift, in most terminals),
select it, and press `m` again.

## How it works

- **`game.rs`** — the position, the cursor, and legality. [`shakmaty`] supplies
  move generation, so castling, en passant, promotion and mate detection are
  handled properly.
- **`session/`** — pairing, with nothing chess-specific in it, so other games
  can use it. [`iroh`] holds a QUIC connection between the two players,
  hole-punching a direct link where it can and falling back to a relay where it
  can't. See [Pairing](#pairing) below.
- **`net.rs`** — chess over a paired session. One bi-directional stream carries
  newline-delimited text: `move e2e4`, `resign`, `draw`.
- **`ui.rs`** — [`ratatui`] draws. It reads state and never writes it. Its
  `Geometry` picks the largest square size the terminal will take, and is also
  what mouse clicks are tested against, so what you see and what you can click
  cannot drift apart.
- **`app.rs`** — turns keypresses and network events into state changes.
- **`lobby.rs`** — the first screen: the menu, the code box with its
  completion, and friends.
- **`profile.rs`** — your identity, name and friends, kept between runs.
- **`clipboard.rs`** — OSC 52, plus the platform's clipboard tool.

There is no referee. Both peers run the same rules over their own copy of the
position, and a move that does not check out locally is rejected rather than
applied, so neither side has to trust the other's arithmetic.

## Pairing

A code is a number and three words from the BIP39 English list, which comes to
about 40 bits: short enough to read out, too many to guess.

1. **Rendezvous.** Both sides stretch the code with Argon2id (64 MiB) into the
   same ed25519 keypair. The host signs a [pkarr] record with it, naming its
   iroh endpoint, and publishes that to the Mainline DHT, BitTorrent's
   network of millions of nodes. The joiner derives the same public key and
   looks the record up. Nobody runs a server for this. Publishing and looking
   up each take a few seconds; a joiner who arrives before the record has
   spread keeps asking for a minute. The stretch is what keeps the codes
   short: finding live games by walking the code space would mean doing it
   for every possible code.
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
the rest. iroh authenticates both ends of every connection, so an invite
needs no code: the host sees who is really dialling, and only asks its player
about friends it already has. Anyone else, or anyone who calls while a game is
on, is turned away with a reason the other side can show (`busy`, `unknown`,
`declined`).

One endpoint serves the whole run, and one listener answers everything that
dials it. What it does depends on where the player is: in the lobby it passes
friends' invites on, while hosting it pairs by code, and during a game it
turns everyone away.

The profile lives in `tui-tui` under your config directory
(`~/Library/Application Support` on macOS, `~/.config` on Linux), or in
`$TUI_TUI_HOME` if set:

- `identity.key` — the secret key, readable only by you
- `profile.json` — your name and friends
- `identity.lock` — held while running

A second copy started while the first holds the lock runs as a guest, with a
throwaway key and nothing saved, rather than answering to the same id. To run
two players on one machine, give the second its own `TUI_TUI_HOME`.

## Tests

`cargo test` covers the rules (castling's rook-square quirk, promotion,
en passant, mate), checks that every square hit-tests correctly at four
terminal sizes in both orientations along with the click and drag gestures,
reads the sprites back out of a rendered buffer to confirm all six pieces are
drawn and are told apart from each other, and stands up two real iroh endpoints
in one process to play moves between them. The pairing tests check that codes
parse the way people retype them, that a wrong code is refused and the host
gives up after three, and that a joiner backs out of a game it does not have.
The lobby tests cover the menu, typing and pasting codes, Tab completion,
flagging bad words as they are typed, clicking items, friends, renaming and
answering invites. The invite tests cover accepting, refusing, a busy player,
an unknown game and a withdrawn invite; the profile tests cover the identity
surviving a restart, the lock, the key's permissions, and saving friends.

`cargo test -- --ignored` also runs pairing over the real DHT, publishing a
code and looking it up. It needs the internet and takes around ten seconds.

`cargo run --example render` prints the UI at several sizes as plain text,
which is the quickest way to iterate on layout.

[`shakmaty`]: https://docs.rs/shakmaty
[`iroh`]: https://docs.rs/iroh
[pkarr]: https://pkarr.org
[`ratatui`]: https://docs.rs/ratatui

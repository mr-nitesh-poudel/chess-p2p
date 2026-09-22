# chess-p2p

Terminal chess for two people, played over a direct peer-to-peer connection.
No server to run, no account, no port forwarding — one of you shares a code,
the other pastes it in.

```
cargo run --release -- host          # prints a code, waits
cargo run --release -- join <code>   # your opponent runs this
cargo run --release                  # or just share a keyboard
```

The host plays white, the joiner plays black.

## Controls

Click a piece and click where it goes, or drag it there — whichever you prefer.
Right-click puts a piece back down. Everything works from the keyboard too:

| key | |
|---|---|
| arrows / `hjkl` | move the cursor |
| `enter` / `space` | pick a piece up, or put it down |
| `f` | flip the board |
| `p` | cycle piece style: sprites, big letters, art, figurines, letters |
| `m` | turn mouse reporting off (see below) |
| `r` | resign (confirm with `y`) |
| `d` | offer or accept a draw |
| `q` / `esc` | quit |

Promotion opens a prompt: click a piece, or `←`/`→` then `enter`, or press
`q` `r` `b` `n`.

## Pieces

The board sizes itself to your terminal, from 3x1 squares up to 11x5, and draws
the best pieces the square can carry.

At 7x3 and above they are proper sprites. Every cell is drawn as `▀`, whose
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
you cannot drag-select your share code. Press `m` to hand the mouse back (or
hold shift, in most terminals), copy it, and press `m` again.

## How it works

- **`game.rs`** — the position, the cursor, and legality. [`shakmaty`] supplies
  move generation, so castling, en passant, promotion and mate detection are
  handled properly.
- **`net.rs`** — [`iroh`] holds a QUIC connection between the two players,
  hole-punching a direct link where it can and falling back to a relay where it
  can't. One bi-directional stream carries newline-delimited text: `move e2e4`,
  `resign`, `draw`.
- **`ui.rs`** — [`ratatui`] draws. It reads state and never writes it. Its
  `Geometry` picks the largest square size the terminal will take, and is also
  what mouse clicks are tested against, so what you see and what you can click
  cannot drift apart.
- **`app.rs`** — turns keypresses and network events into state changes.

There is no referee. Both peers run the same rules over their own copy of the
position, and a move that does not check out locally is rejected rather than
applied, so neither side has to trust the other's arithmetic.

The code you share is your iroh endpoint id — a public key. iroh's discovery
turns it into a reachable address, which is why `join` needs nothing else.

## Tests

`cargo test` covers the rules (castling's rook-square quirk, promotion,
en passant, mate), checks that every square hit-tests correctly at four
terminal sizes in both orientations along with the click and drag gestures,
reads the sprites back out of a rendered buffer to confirm all six pieces are
drawn and are told apart from each other, and stands up two real iroh endpoints
in one process to play moves between them.

`cargo run --example render` prints the UI at several sizes as plain text,
which is the quickest way to iterate on layout.

[`shakmaty`]: https://docs.rs/shakmaty
[`iroh`]: https://docs.rs/iroh
[`ratatui`]: https://docs.rs/ratatui

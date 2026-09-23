# tui-tui

Two people, two terminals, one game. No server, no account, no port forwarding.

You read out a code like `42-tiger-marble-ocean`, your friend types it in, and
you are playing. The code is not an address on somebody's server — it *is* the
address, stretched into a keypair and looked up on a public DHT. Chess is the
first game; the lobby, pairing and friends are shared, so more can follow.

```
╭───────────────── tui-tui ──────────────────╮
│                                            │
│   Game        ‹ Chess ›                    │
│                                            │
│   Host a game                              │
│   get a code to send your opponent         │
│   Join a game                              │
│   type in the code your opponent sent      │
│   Play on one keyboard                     │
│   two players taking turns                 │
│                                            │
│ friends                                    │
│ ▸ alice              3 games · 3h ago      │
│   bob                1 game · yesterday    │
│                                            │
│   Your name                                │
│   ace — what friends see                   │
│   Quit                                     │
│                                            │
│ code 42-tiger-marble-ocean                 │
╰────────────────────────────────────────────╯
```

## Install

```
brew install mr-nitesh-poudel/tap/tuitui
```

Or from a clone, with `cargo install --path .`.

## Play

```
tuitui                            # the lobby
tuitui host                       # or skip it: host a game,
tuitui join 42-tiger-marble-ocean # join one,
tuitui local                      # or share a keyboard
tuitui play chess                 # the lobby, on a game of your choosing
```

Every command but `join` takes an optional game. Joining never needs one: you
get whatever the host is playing. Host plays white.

Codes are forgiving. Any case, spaces instead of dashes, and four letters a
word is plenty — `42 tige marb ocea` gets you there. Tab finishes a word, and
a word that isn't in the list is flagged while you type it. Paste the whole
`tuitui join ...` command if that's what landed in your clipboard.

## At the board

Click a piece and click where it goes, or drag it. Right-click puts it back
down. The keyboard does everything too:

| key | |
|---|---|
| arrows / `hjkl` | move the cursor |
| `enter` / `space` | pick up, put down |
| `f` | flip the board |
| `p` | cycle piece style |
| `m` | hand the mouse back to your terminal |
| `c` | copy your share code |
| `r` / `d` | resign / offer a draw |
| `q` / `esc` | leave (it asks first) |

Promotion opens a prompt: click, or `←`/`→` and `enter`, or just press
`q` `r` `b` `n`.

Checkmate is not a status line. The board goes dark, the square flashes red,
and the losing king topples over away from whatever mated it before the
verdict is spelled out across the board. Resigning lays your king down gently
instead. Any key puts the board back.

## Friends

Anyone you play turns up in the lobby under **friends**, and challenging one
takes no code at all — their lobby just asks them to accept. Leaving a game
drops you back with your last opponent already selected, so a rematch is one
keypress. `x` forgets someone.

## Pieces

The board sizes itself to your terminal, from 3x1 squares up to 11x5, and
draws the best pieces the square can carry: vector silhouettes stamped dot by
dot with Unicode octants, half-block sprites below that, then character art,
figurines and plain letters. A sliding piece moves a dot at a time, not a cell
at a time. If your terminal can't draw octants you'll see `�` — press `p` once
for braille, which everything can draw.

## How it finds your friend

A code is a number and three words: about 40 bits, short enough to read out
and far too many to guess at one try per connection.

1. Both sides stretch the code with Argon2id into the same keypair. The host
   signs a record naming its address and publishes it to the Mainline DHT —
   BitTorrent's, millions of nodes, nobody's server. The joiner derives the
   same key and looks it up.
2. Both prove they hold the code with SPAKE2, bound to both endpoint ids. A
   stranger gets one guess per connection, and three wrong ones and the host
   stops listening.
3. The host names its game, the joiner accepts or backs out.

Codes expire after an hour, or the moment the host leaves.

There is no referee. Both sides run the same rules over their own copy of the
position, so nobody has to trust the other's arithmetic.

## Hacking on it

`hub.rs` runs the loop and knows nothing about chess. `games/` holds the
games, each one a `Play`: take keys, clicks and the opponent's lines, and
draw. `session/` does pairing, `lobby/` is the first screen, `profile.rs`
remembers who you are.

Adding a game means a module under `games/`, a `Play` implementation and a
line in `Kind`. Pairing, friends, invites and the lobby pick it up for free.
`unsafe` is forbidden crate-wide, and release builds keep overflow checks on.

[DESIGN.md](DESIGN.md) has the long version: how the pieces are drawn, what
the two sides say to each other, and how pairing works in detail.

## Tests

```
cargo test                  # rules, hit-testing, drawing, two real endpoints talking
cargo test -- --ignored     # pairing over the real DHT, needs the internet
cargo run --example render  # prints the UI at several sizes, for layout work
```

One test draws and clicks every screen at every terminal size from 0x0 up to
a maximised window, because a screen too small for a board is where the
arithmetic gives out.

## Licence

MIT or Apache-2.0, your pick.

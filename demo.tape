# A game of fool's mate, from the lobby to the verdict.
#
#   brew install vhs
#   vhs demo.tape      # writes demo.gif
#
# The cursor starts on e2, so every move below is counted from wherever
# the last one left it. Change a move and you must recount the rest.

Output demo.gif

Set FontSize 18
Set Width 1100
Set Height 640
Set Padding 20
Set TypingSpeed 80ms

Hide
Type "clear"
Enter
Show

Type "tuitui"
Enter
Sleep 2.5s

# Down past Join to "Play on one keyboard".
Down 2
Sleep 800ms
Enter
Sleep 1.5s

# 1. f3 — cursor starts on e2
Right
Sleep 400ms
Enter
Sleep 400ms
Up
Enter
Sleep 1.2s

# 1... e5
Left
Up 4
Sleep 400ms
Enter
Sleep 400ms
Down 2
Enter
Sleep 1.2s

# 2. g4
Right 2
Down 3
Sleep 400ms
Enter
Sleep 400ms
Up 2
Enter
Sleep 1.2s

# 2... Qh4# — the queen runs the diagonal and it is mate
Left 3
Up 4
Sleep 600ms
Enter
Sleep 600ms
Right 4
Down 4
Sleep 400ms
Enter

# The shudder, the topple, and the verdict.
Sleep 5s
Space
Sleep 2s

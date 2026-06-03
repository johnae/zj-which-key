# UI choreography for the demo GIF. Regenerate with:  ./record-demo.sh
#
# record-demo.sh builds the wasm and runs this with a pre-approved permission
# cache, so no approval prompt interrupts the recording. The {{CONFIG}} token is
# substituted by record-demo.sh; don't run `vhs demo.tape` directly.

Output demo.gif

Set Shell "bash"
Set FontSize 14
Set Width 1200
Set Height 600
Set Padding 10

Hide
Type "zellij --config {{CONFIG}}"
Enter
Sleep 3s
Ctrl+l
Sleep 1s
Show

# Enter pane mode -> popup fades in after the idle delay with pane-mode keys.
Sleep 1s
Ctrl+p
Sleep 2.5s
Escape
Sleep 1.5s

# Enter tab mode -> popup shows tab-mode keys.
Ctrl+t
Sleep 2.5s
Escape
Sleep 1.5s

# Open the searchable all-modes browser and filter live.
Ctrl+y
Sleep 1.5s
Type "pane"
Sleep 2s
Down
Down
Sleep 1.5s
Escape
Sleep 1s

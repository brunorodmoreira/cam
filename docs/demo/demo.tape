# The demo at the top of the README. Record it with `sh docs/demo/record.sh`,
# which sets up the throwaway home this expects.
Output docs/demo.gif

Set Shell zsh
Set FontSize 16
Set Width 1120
Set Height 520
Set Padding 24
Set WindowBar Colorful
Set TypingSpeed 45ms

Env HOME "/Users/Shared/me"
# Homebrew last: vhs finds ttyd through this PATH.
Env PATH "/Users/Shared/me/bin:/usr/bin:/bin:/opt/homebrew/bin"
Env SHELL "/bin/zsh"

# vhs starts zsh without its startup files, so set up what ~/.zshrc would.
Hide
Type `PROMPT='%F{cyan}%~%f $ '`
Enter
Type `eval "$(cam init zsh)"`
Enter
Type "cd ~ && clear"
Enter
Sleep 500ms
Show

# Add an account, sharing your instructions and skills with it.
Type "cam add work"
Sleep 400ms
Enter
Sleep 2s
Down
Sleep 300ms
Space
Sleep 300ms
Down
Sleep 300ms
Space
Sleep 1s
Enter
Sleep 4s
Ctrl+L

# Use it in ~/Work and every folder below.
Type "cam use work --dir ~/Work"
Sleep 300ms
Enter
Sleep 1.5s
Type "cd ~/Work/api && cam which"
Sleep 300ms
Enter
Sleep 2s
Type "claude"
Sleep 300ms
Enter
Sleep 2s

# Anywhere else, it's your own login.
Type "cd ~ && claude"
Sleep 300ms
Enter
Sleep 2s
Type "cam list"
Sleep 300ms
Enter
Sleep 4s

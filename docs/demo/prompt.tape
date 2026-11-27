# The Starship prompt in docs/usage.md. Record it with
# `sh docs/demo/record.sh docs/demo/prompt.tape`, which sets up the throwaway
# home this expects: an account `work` used in ~/Work, and Starship's config.
Output docs/prompt.gif

Set Shell zsh
Set FontSize 16
Set Width 1120
Set Height 400
Set Padding 24
Set WindowBar Colorful
Set TypingSpeed 45ms

Env HOME "/Users/Shared/me"
# Homebrew last: vhs finds ttyd, and the prompt starship, through this PATH.
Env PATH "/Users/Shared/me/bin:/usr/bin:/bin:/opt/homebrew/bin"
Env SHELL "/bin/zsh"

# vhs starts zsh without its startup files, so set up what ~/.zshrc would.
Hide
Type `eval "$(cam init zsh)"`
Enter
Type `eval "$(starship init zsh)"`
Enter
Type "cd ~ && clear"
Enter
Sleep 500ms
Show

# In ~/Work, the prompt names the account plain `claude` will use.
Sleep 1s
Type "cd ~/Work/api"
Sleep 300ms
Enter
Sleep 1.5s
Type "claude"
Sleep 300ms
Enter
Sleep 2s

# Anywhere else, `claude` is your own login, and the prompt says nothing.
Type "cd ~/oss"
Sleep 300ms
Enter
Sleep 1.5s
Type "claude"
Sleep 300ms
Enter
Sleep 3s

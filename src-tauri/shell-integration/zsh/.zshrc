_twominal_wrapper_zdotdir=$ZDOTDIR
ZDOTDIR=${TWOMINAL_USER_ZDOTDIR:-$HOME}
if [[ $HISTFILE == "$_twominal_wrapper_zdotdir/.zsh_history" ]]; then
  HISTFILE="$ZDOTDIR/.zsh_history"
fi
if [[ -r $ZDOTDIR/.zshrc ]]; then
  source $ZDOTDIR/.zshrc
fi
TWOMINAL_USER_ZDOTDIR=${ZDOTDIR:-$HOME}
if [[ -r ${TWOMINAL_INTEGRATION_SCRIPT:-} ]]; then
  source $TWOMINAL_INTEGRATION_SCRIPT
fi
ZDOTDIR=$TWOMINAL_USER_ZDOTDIR
export ZDOTDIR
unset TWOMINAL_USER_ZDOTDIR TWOMINAL_INTEGRATION_SCRIPT _twominal_wrapper_zdotdir

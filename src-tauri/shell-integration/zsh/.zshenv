_twominal_wrapper_zdotdir=$ZDOTDIR
typeset -g _TWOMINAL_BOOTSTRAP_NONCE=${TWOMINAL_SHELL_INTEGRATION_NONCE:-}
unset TWOMINAL_SHELL_INTEGRATION_NONCE
ZDOTDIR=${TWOMINAL_USER_ZDOTDIR:-$HOME}
if [[ -r $ZDOTDIR/.zshenv ]]; then
  source $ZDOTDIR/.zshenv
fi
TWOMINAL_USER_ZDOTDIR=${ZDOTDIR:-$HOME}
ZDOTDIR=$_twominal_wrapper_zdotdir
export TWOMINAL_USER_ZDOTDIR ZDOTDIR
unset _twominal_wrapper_zdotdir

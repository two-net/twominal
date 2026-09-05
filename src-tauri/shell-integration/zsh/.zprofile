_twominal_wrapper_zdotdir=$ZDOTDIR
ZDOTDIR=${TWOMINAL_USER_ZDOTDIR:-$HOME}
if [[ -r $ZDOTDIR/.zprofile ]]; then
  source $ZDOTDIR/.zprofile
fi
TWOMINAL_USER_ZDOTDIR=${ZDOTDIR:-$HOME}
ZDOTDIR=$_twominal_wrapper_zdotdir
export TWOMINAL_USER_ZDOTDIR ZDOTDIR
unset _twominal_wrapper_zdotdir

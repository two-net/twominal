# Twominal shell integration. This file only emits semantic terminal markers.
if [[ -n ${TWOMINAL_SHELL_INTEGRATION_ACTIVE:-} ]]; then
  return
fi
TWOMINAL_SHELL_INTEGRATION_ACTIVE=1
_TWOMINAL_INTEGRATION_NONCE=${TWOMINAL_SHELL_INTEGRATION_NONCE:-}
unset TWOMINAL_SHELL_INTEGRATION_NONCE

if [[ ${TWOMINAL_BASH_LOGIN:-0} == 1 ]]; then
  if [[ -r /etc/profile ]]; then
    . /etc/profile
  fi
  if [[ -r ~/.bash_profile ]]; then
    . ~/.bash_profile
  elif [[ -r ~/.bash_login ]]; then
    . ~/.bash_login
  elif [[ -r ~/.profile ]]; then
    . ~/.profile
  fi
  unset TWOMINAL_BASH_LOGIN
elif [[ -r ~/.bashrc ]]; then
  . ~/.bashrc
fi

_TWOMINAL_PROMPT_SUFFIX="\\[\\e]133;B;${_TWOMINAL_INTEGRATION_NONCE}\\a\\]"

_twominal_hex_encode() {
  local LC_ALL=C input=$1 output='' byte index
  for (( index = 0; index < ${#input}; index++ )); do
    printf -v byte '%02x' "'${input:index:1}"
    output+=$byte
  done
  printf '%s' "$output"
}

_twominal_prompt_command() {
  local command_status=$?
  printf '\e]133;D;%d;%s\a\e]133;A;%s\a' "$command_status" "$_TWOMINAL_INTEGRATION_NONCE" "$_TWOMINAL_INTEGRATION_NONCE"
  printf '\e]133;P;CwdHex=%s;%s\a' "$(_twominal_hex_encode "$PWD")" "$_TWOMINAL_INTEGRATION_NONCE"

  if [[ $PS1 == *"$_TWOMINAL_PROMPT_SUFFIX" ]]; then
    PS1=${PS1%"$_TWOMINAL_PROMPT_SUFFIX"}
  fi
  PS1="${PS1}${_TWOMINAL_PROMPT_SUFFIX}"
}

case "$(declare -p PROMPT_COMMAND 2>/dev/null)" in
  'declare -a'*) PROMPT_COMMAND+=("_twominal_prompt_command") ;;
  *) PROMPT_COMMAND="${PROMPT_COMMAND:+${PROMPT_COMMAND};}_twominal_prompt_command" ;;
esac

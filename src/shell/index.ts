export {
  DEFAULT_AUTOSUGGESTION_LIMIT,
  MAX_AUTOSUGGESTION_LIMIT,
  bestAutosuggestion,
  rankAutosuggestions,
  type Autosuggestion,
  type AutosuggestionOptions,
} from "./autosuggestions";
export {
  DEFAULT_HISTORY_CAPACITY,
  HistoryNavigator,
  HistoryStore,
  MAX_HISTORY_CAPACITY,
  MAX_HISTORY_COMMAND_BYTES,
  normalizeHistoryCommand,
  type HistoryEntry,
  type HistoryRecordOptions,
  type HistorySource,
  type HistoryStoreOptions,
} from "./history";
export {
  tokenizeShellLine,
  type ActiveShellToken,
  type ShellQuote,
  type ShellToken,
  type ShellTokenKind,
  type TokenizedShellLine,
} from "./shellTokenizer";
export {
  buildCompletionChoices,
  completionRequestForLine,
  syntaxContext,
  type CompletionCandidateKind,
  type CompletionChoice,
  type CompletionRequest,
  type CompletionRequestKind,
  type NativeCompletionCandidate,
} from "./completions";
export {
  ShellClientError,
  TauriShellClient,
  normalizeShellError,
  type ShellClient,
} from "./shellClient";
export {
  PromptInputModel,
  countGraphemes,
  type PromptInputPhase,
  type PromptInputSnapshot,
  type PromptInputUpdate,
} from "./PromptInputModel";
export {
  ShellExperienceController,
  parseCwdProperty,
  type ShellExperienceControllerOptions,
  type ShellExperiencePhase,
  type ShellExperienceStatus,
} from "./ShellExperienceController";

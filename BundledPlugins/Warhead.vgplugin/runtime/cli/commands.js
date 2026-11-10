import { BOLD, RESET } from './style.js';
export const USAGE = `${BOLD}commands${RESET}
  /help                 this text
  /paste | /blob        paste a multiline prompt; finish with /end
  /edit [draft]          write a multiline prompt in your editor
  /edit --resume         reopen a draft saved after editor failure
  /attach <file>        attach a text, source, or binary file
  /attachments             list queued attachments
  /attachments show n [tail] preview the start or end of one
  /attachments rename n name label a queued attachment
  /attachments move n m   reorder queued attachments
  /attachments stats      show total queued size and estimated tokens
  /detach [n|name|last]  remove an attachment; confirm before clearing all
  /undo-attachment       remove the most recently queued attachment
  /retry [--restart]    continue safely; --restart replays the original task
  /undo                 remove the last turn
  /save <name>          save this session
  /load <name>          load a saved session
  /recover [file]       recover the latest session for this directory
  /restore <file>       restore a file recovery checkpoint
  /export <file>        export conversation as Markdown
  /verbose              toggle expanded tool output
  /status               show model, provider, usage, and context state
  /connect <host>       add/select a provider endpoint
  /commands             show the command palette
  /quit                 quit (alias for /exit)
${BOLD}  model${RESET}
  /models               list local weights and server models (no inference probes)
  /models select <n>    select a downloaded model by number
  /model                show the resolved model, provider, and probe history
  /model use <tag>      use a server tag or a direct mlx:/gguf:/ollama: reference
  /model show <tag>     show Ollama model metadata
  /model pull <tag>     pull a model from Ollama
  /model delete <tag>   delete a local model
  /model copy <from> <to> copy a model tag
  /model create <name> <spec.json> create a model from a JSON specification
  /model version         show the active Ollama version
  /model ps              show running Ollama models
  /model pick [filter]  probe matching installed models and choose one
  /model forget [tag]   drop a candidate and forget its probe result
${BOLD}  providers${RESET}
  /provider             list providers and show the active one
  /provider add <name> <host> [--key-env VAR | --key VALUE]
  /provider use <name>  switch; re-resolves a model for the new host
  /provider default     back to OLLAMA_HOST or localhost:11434
  /provider remove <name>
${BOLD}  team (optional)${RESET}
  /team                 show the lead model and configured advisors
  /team add <name> <provider> <model> [specialty]
  /team remove <name>    remove an advisor
  /team on | off        enable or disable advice on new tasks
  /team check           check both servers without loading models
${BOLD}  mcp${RESET}
  /mcp                  show connected remote MCP servers
${BOLD}  permissions${RESET}
  /goal <text>          set a persistent goal the model works toward
  /permissions            show permission policy
  /permissions mode …     set default asking: strict | balanced | relaxed
  /permissions bash …     set bash policy to ask | allow | allowlist
  /permissions allow|deny <tool>
  /permissions reset      clear session-scoped grants
  /tools                list available tools
  /todos                show the current task list
  /transcripts          show transcript location and usage
  /transcripts size | clear
  /clear                forget conversation history
  /exit                 quit`;
export const SLASH_COMMANDS = new Set([
    'help', 'verbose', 'retry', 'undo', 'save', 'load', 'export', 'recover', 'restore',
    'paste', 'blob', 'edit', 'attach', 'attachments', 'detach', 'undo-attachment', 'status', 'mcp', 'commands', 'palette',
    'tools', 'todos', 'clear', 'transcripts', 'models', 'connect',
    'provider', 'model', 'team', 'permissions', 'exit', 'quit', 'goal',
    'copy', 'copy-last',
]);
export const COMMAND_PALETTE = [
    ['help', 'show all commands'],
    ['recover', 'recover the latest interrupted session'],
    ['restore <file>', 'restore a file recovery checkpoint'],
    ['commands', 'open this command palette'],
    ['paste', 'queue a multiline paste'],
    ['edit', 'write a multiline prompt in your editor'],
    ['edit --resume', 'reopen a saved editor draft'],
    ['attach <file>', 'queue a file attachment'],
    ['attachments', 'list queued attachments'],
    ['attachments show n [tail]', 'preview the start or end'],
    ['attachments rename n name', 'label a queued attachment'],
    ['attachments move n m', 'reorder queued attachments'],
    ['attachments stats', 'show queued size and context estimate'],
    ['detach [n|name|last]', 'remove one attachment; confirm before clearing all'],
    ['undo-attachment', 'remove the most recently queued attachment'],
    ['retry', 'retry the last prompt'],
    ['undo', 'remove the last completed turn'],
    ['status', 'show model, context, and usage'],
    ['copy [last|screen|all]', 'copy text to clipboard/file'],
    ['copy-last', 'copy the last reply to clipboard/file'],
    ['models', 'list downloaded models'],
    ['model use <tag>', 'switch models'],
    ['provider', 'inspect or switch endpoints'],
    ['team', 'configure optional cooperation across model servers'],
    ['permissions', 'inspect or change tool policy'],
    ['verbose', 'toggle expanded tool output'],
    ['clear', 'forget conversation history'],
    ['goal <text>', 'set a persistent goal the model works toward'],
    ['exit', 'leave Warhead'],
];
const COMMAND_NAMES = [...SLASH_COMMANDS].sort();
export function completeSlashCommand(line) {
    if (!line.startsWith('/'))
        return [[], line];
    const token = line.slice(1).split(/\s/, 1)[0].toLowerCase();
    const matches = COMMAND_NAMES.filter(name => name.startsWith(token)).map(name => `/${name}`);
    return [matches.length > 0 ? matches : COMMAND_NAMES.map(name => `/${name}`), line];
}
export function closestSlashCommand(input) {
    const target = input.toLowerCase();
    let best = null;
    for (const name of COMMAND_NAMES) {
        const previous = Array.from({ length: name.length + 1 }, (_, i) => i);
        for (let row = 1; row <= target.length; row++) {
            const current = [row];
            for (let col = 1; col <= name.length; col++) {
                current[col] = target[row - 1] === name[col - 1]
                    ? previous[col - 1]
                    : Math.min(previous[col - 1] + 1, previous[col] + 1, current[col - 1] + 1);
            }
            previous.splice(0, previous.length, ...current);
        }
        const distance = previous[name.length];
        if (!best || distance < best.distance)
            best = { name, distance };
    }
    if (!best)
        return null;
    const limit = target.length <= 4 ? 1 : 2;
    return best.distance <= limit ? `/${best.name}` : null;
}

/**
 * Detect an assistant that is describing the same future tool action instead of
 * executing it. This is deliberately narrower than general repetition
 * detection: quoted examples, long explanations, and actual tool use should not
 * be mistaken for a stalled agent.
 *
 * The model stream is scanned in bounded windows, then the agent gets exactly
 * one recovery attempt. No prompt-only rule can reliably enforce that ceiling.
 */
const WINDOW_CHARS = 4096;
const INTENT = /\b(?:i['’]ll|i\s+will|i\s+should|i\s+need\s+to|let\s+me|i['’]m\s+going\s+to|i\s+am\s+going\s+to)\s+(?:(?:just|now|actually|instead|first|try\s+to)\s+){0,3}(?:use|run|execute|call|try|check|inspect|read|list|open)\s+(?:the\s+)?[`"']?([a-z][a-z0-9._/:-]{0,63})/gi;
function visibleProse(text) {
    // Never interpret code samples or quoted explanations as the model's plan.
    return text.slice(-WINDOW_CHARS)
        .replace(/```[^]*?```/g, ' ')
        .replace(/```[^]*$/g, ' ')
        .replace(/^\s*>.*$/gm, ' ');
}
function intentions(text) {
    const found = [];
    for (const match of visibleProse(text).matchAll(INTENT)) {
        // The capture includes any punctuation that directly follows the target
        // (for example the full stop in "I'll use ls."). Sentence punctuation is
        // not part of the command or path, so compare the normalised target.
        found.push(match[1].toLowerCase().replace(/[.,;:!?]+$/, ''));
    }
    return found;
}
/** Return a repeated target (e.g. "ls"), or null for ordinary prose. */
export function repeatedPlanningAction(text) {
    const prose = visibleProse(text);
    const actions = intentions(prose);
    if (actions.length < 2)
        return null;
    const last = actions.at(-1);
    // Consecutive re-statements of the same intention are the clearest signal.
    if (actions.slice(-3).length === 3 && actions.slice(-3).every(a => a === last))
        return last;
    // A ↔ B oscillation is just as unproductive as A → A → A.
    if (actions.length >= 4) {
        const tail = actions.slice(-4);
        if (tail[0] === tail[2] && tail[1] === tail[3])
            return last;
    }
    // Some models emit "Actually ... Wait ... Actually ..." with only two
    // complete intentions before the next self-correction. Need two distinct
    // self-correction markers to avoid blocking ordinary explanatory writing.
    const corrections = prose.match(/\b(?:actually|wait|hold on|instead|on second thought)\b/gi) ?? [];
    if (actions.filter(a => a === last).length >= 2 && corrections.length >= 2 &&
        prose.length <= 1200)
        return last;
    return null;
}
/** Only short unfulfilled plans qualify; not a response explaining what to do. */
export function unexecutedPlanningAction(text) {
    const prose = visibleProse(text).trim();
    if (!prose || prose.length > 280)
        return null;
    const matches = intentions(prose);
    if (!matches.length)
        return null;
    if (/\b(?:i (?:ran|used|found|checked|finished)|the (?:output|result) (?:is|was)|completed successfully)\b/i.test(prose))
        return null;
    // An action-only completion that never invoked a tool is not task completion.
    return matches.at(-1);
}
/** Avoid forcing tool use when a person requested an explanation or a plan. */
export function expectsExecution(userText) {
    const request = userText.trim();
    if (/^(?:how|why|what|which|where|when|explain|describe|tell me|outline|suggest)\b/i.test(request))
        return false;
    return /\b(?:perform|do|run|execute|fix|implement|edit|repair|build|test|inspect|check|analy[sz]e|review|make|create|update|change|investigate|continue|solve|install|add|remove|refactor|find|complete|debug|work on)\b/i.test(request);
}

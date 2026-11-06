/** Model request preparation is shared by the CLI, Studio, and loop tests. */
import { compactIfNeeded, conversationChars } from '../context/compact.js';
import { assertRequestFits, inputBudgetChars, requestChars } from '../context/budget.js';
import { composeSystemPrompt, fetchModelContextLength, fetchModelSystemPrompt, MAX_AUTO_CONTEXT_TOKENS } from '../model/modelfile.js';
import { systemPrompt } from '../prompt/system.js';
import { resolveLocalModel } from '../model/local-models.js';
import { nativeToolsForTurn } from './native-tool-budget.js';
export async function prepareTurn(options) {
    const { model, registry, config, ctx, signal, userText } = options;
    const [defaults, modelContextTokens] = await Promise.all([
        fetchModelSystemPrompt(model, signal), fetchModelContextLength(model, signal),
    ]);
    signal.throwIfAborted();
    // The system prompt and request-size estimator must describe the very same
    // tools that the execution loop actually advertises to native small models.
    const source = resolveLocalModel(model);
    // For larger local models or when context allows, prefer full toolset for better agentic/reverse engineering capability
    const selected = source && modelContextTokens !== null && modelContextTokens <= 6144
        ? nativeToolsForTurn(registry.tools, userText)
        : registry.tools;
    const system = { role: 'system', content: composeSystemPrompt(defaults, systemPrompt({ cwd: ctx.cwd, tools: selected, config })) };
    const user = { role: 'user', content: userText };
    const tools = options.conversational ? [] : selected.map(tool => tool.def);
    // Input alone must fit; summarizing past turns cannot make a huge paste fit.
    assertRequestFits([system, user], tools, modelContextTokens);
    let history = options.history;
    const before = requestChars([system, ...history, user], tools);
    const threshold = Math.min(config.limits.compactThresholdChars, inputBudgetChars(modelContextTokens));
    const framing = requestChars([system, user], tools) + JSON.stringify(history).length - conversationChars(history);
    if (before > threshold) {
        const compact = async (keepRecent) => compactIfNeeded(history, {
            model, thresholdChars: threshold, reservedChars: framing, keepRecent, signal,
            numCtx: Math.min(modelContextTokens ?? MAX_AUTO_CONTEXT_TOKENS, MAX_AUTO_CONTEXT_TOKENS),
        });
        let result = await compact(config.limits.compactKeepRecent);
        if (!result.compacted && config.limits.compactKeepRecent > 1)
            result = await compact(1);
        history = result.messages;
        options.onContext?.(before, requestChars([system, ...history, user], tools), result.error);
    }
    const messages = [system, ...history, user];
    assertRequestFits(messages, tools, modelContextTokens);
    return { messages, modelContextTokens };
}

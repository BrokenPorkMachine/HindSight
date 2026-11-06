import { ArgError, optInt, reqString } from './args.js';
const MAX_RESULTS = 8;
const MAX_QUERY = 300;
function stripHtml(s) {
    return s.replace(/<[^>]*>/g, ' ').replace(/&amp;/g, '&').replace(/&quot;/g, '"')
        .replace(/&#x27;/g, "'").replace(/\s+/g, ' ').trim();
}
export const webSearch = {
    mutates: false,
    def: {
        name: 'web_search',
        description: 'Search the web and return a small set of titled links and snippets. Use for current or niche information; treat results as leads and prefer primary sources. Network access may require approval.',
        parameters: {
            type: 'object',
            properties: {
                query: { type: 'string', description: 'Focused search query' },
                max_results: { type: 'integer', description: `Maximum results, 1-${MAX_RESULTS}` },
            },
            required: ['query'],
        },
    },
    async run(args) {
        try {
            const query = reqString(args, 'query').slice(0, MAX_QUERY);
            const count = Math.min(MAX_RESULTS, Math.max(1, optInt(args, 'max_results', 1) ?? 5));
            const base = process.env['WARHEAD_SEARCH_URL'] ?? 'https://html.duckduckgo.com/html/';
            const url = `${base}${base.includes('?') ? '&' : '?'}q=${encodeURIComponent(query)}`;
            const res = await fetch(url, {
                headers: { 'user-agent': 'warhead-web-search/1.0' },
                signal: AbortSignal.timeout(20_000),
            });
            if (!res.ok)
                return { ok: false, output: '', error: `search returned HTTP ${res.status}` };
            const html = await res.text();
            const rows = [];
            const re = /<a[^>]+class="result__a"[^>]+href="([^"]+)"[^>]*>([\s\S]*?)<\/a>[\s\S]*?<a[^>]+class="result__snippet"[^>]*>([\s\S]*?)<\/a>/g;
            let match;
            while (rows.length < count && (match = re.exec(html))) {
                rows.push(`${rows.length + 1}. ${stripHtml(match[2] ?? '')}\n   ${match[1] ?? ''}\n   ${stripHtml(match[3] ?? '')}`);
            }
            return { ok: true, output: rows.length ? `Search results for: ${query}\n\n${rows.join('\n\n')}` : `No results found for: ${query}` };
        }
        catch (err) {
            return { ok: false, output: '', error: err instanceof ArgError ? err.message : String(err) };
        }
    },
};

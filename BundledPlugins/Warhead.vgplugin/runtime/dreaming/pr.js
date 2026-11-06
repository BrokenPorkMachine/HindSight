export class DreamPrPublisher {
    enabled = false;
    isEnabled() { return this.enabled; }
    async publishDraft(_req) {
        if (!this.enabled)
            return { ok: false, reason: 'dreaming pr publishing not enabled', draft: true };
        // Must never merge - only create/update draft PRs
        return { ok: false, reason: 'dreaming pr publishing not implemented', draft: true };
    }
    async updateDraft(_req) {
        if (!this.enabled)
            return { ok: false, reason: 'dreaming pr publishing not enabled', draft: true };
        return { ok: false, reason: 'dreaming pr publishing not implemented', draft: true };
    }
}

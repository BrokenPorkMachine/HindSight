/**
 * Dreaming manager (D0 stub). Disabled by default; no background worker is started.
 * Activation requires explicit opt-in per scope as per the spec.
 */
export class DreamingManager {
    enabled = false;
    constructor() {
        this.enabled = false;
    }
    isEnabled() {
        return this.enabled;
    }
    enable(_scope) {
        // Explicit opt-in only; activation is not implemented in this stub.
        throw new Error('Dreaming activation is not enabled in this build');
    }
    disable() {
        this.enabled = false;
    }
    async observe() {
        throw new Error('Dreaming observation pipeline not implemented');
    }
}
export const dreaming = new DreamingManager();

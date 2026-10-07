import type { ProviderAdapter } from './providers/adapter';
import { extractGlyimOpsBlocks, isBlockComplete } from './code_extractor';
import { containsDangerousPattern, normalizeLineEndings } from './types';

export class StreamWatcher {
  private observer: MutationObserver | null = null;
  private copyObserver: MutationObserver | null = null;
  private turn = 0;
  private previousResponseText = '';
  private sentHashes = new Set<string>();
  private isWatching = false;
  private pollingTimer: ReturnType<typeof setInterval> | null = null;
  private lastStreaming = false;
  private pendingCheck: Promise<void> = Promise.resolve();
  private completed = false; // prevent duplicate completion

  constructor(
    private adapter: ProviderAdapter,
    private sessionId: string,
    private onOpsReady: (content: string, turn: number) => void,
    private onStreamComplete: (fullResponse: string, turn: number) => void,
    private onDangerousPattern: (content: string, pattern: string) => void,
  ) { }

  start(): void {
    if (this.isWatching) return;
    this.isWatching = true;
    this.completed = false;

    const container = document.querySelector('[role="main"]') ??
      document.querySelector(this.adapter.assistantSelector)?.parentElement ??
      document.body;

    // T162-PATCHED [EXT-6]: debounce the DOM observer at 500 ms. The
    // previous observer ran `serializedCheck` (which computes the full
    // extracted-text SHA-256) on *every* mutation of the streaming
    // response, so a token-by-token stream produced O(n²) work in the
    // provider page's main thread. 500 ms is the spec's minimum
    // debouncing interval (REQ-FUNC-056).
    let debounceHandle: ReturnType<typeof setTimeout> | null = null;
    this.observer = new MutationObserver(() => {
      if (this.adapter.isStreaming()) return;
      if (debounceHandle) return;
      debounceHandle = setTimeout(() => {
        debounceHandle = null;
        void this.serializedCheck();
      }, 500);
    });
    this.observer.observe(container, { childList: true, subtree: true, characterData: true });

    // Polling timer for streaming state (fallback)
    this.pollingTimer = setInterval(() => {
      if (!this.isWatching) return;
      // T163-PATCHED [EXT-7]: rate-limit / server-error detection.
      // `detectError` has been implemented on every provider adapter but
      // was never called — the CAP-RETRY path in the server (REQ-FUNC-
      // 033..040) had no input. Poll it once per tick; when a provider
      // error is present, surface it via `onDangerousPattern` (a generic
      // callback we reuse for out-of-band error signalling) and stop the
      // watcher so it doesn't keep firing.
      const providerErr = this.adapter.detectError?.();
      if (providerErr) {
        this.onDangerousPattern(
          '',
          `provider-error:${providerErr.kind ?? 'unknown'}`,
        );
        this.lastStreaming = false;
        return;
      }
      const streaming = this.adapter.isStreaming();
      if (this.lastStreaming && !streaming && !this.completed) {
        void this.serializedCheck();
        this.handleStreamComplete();
      }
      this.lastStreaming = streaming;
    }, 500);

    // NEW: MutationObserver to detect copy toolbar insertion
    this.copyObserver = new MutationObserver(() => {
      if (this.completed) return;
      const lastMsg = document.querySelector(`${this.adapter.assistantSelector}:last-of-type`);
      if (lastMsg) {
        const copyBtn = lastMsg.querySelector('button[aria-label*="Copy"], button[aria-label*="copy"], [class*="copy"]');
        if (copyBtn) {
          // Copy button appears – streaming is definitely complete
          this.handleStreamComplete();
          this.completed = true;
          this.copyObserver?.disconnect();
        }
      }
    });
    this.copyObserver.observe(document.body, { childList: true, subtree: true });
  }

  stop(): void {
    this.isWatching = false;
    this.observer?.disconnect();
    this.copyObserver?.disconnect();
    this.observer = null;
    this.copyObserver = null;
    if (this.pollingTimer) clearInterval(this.pollingTimer);
    this.pollingTimer = null;
  }

  resetForNewTurn(): void {
    this.turn++;
    this.previousResponseText = '';
    this.sentHashes.clear();
    this.completed = false;
  }

  private async serializedCheck(): Promise<void> {
    this.pendingCheck = this.pendingCheck.then(() => this.checkForCompleteBlocks());
    await this.pendingCheck;
  }

  private async checkForCompleteBlocks(): Promise<void> {
    try {
      const text = this.adapter.getAssistantText();
      if (!text || text === this.previousResponseText) return;
      this.previousResponseText = text;
      const blocks = extractGlyimOpsBlocks(normalizeLineEndings(text));
      for (const block of blocks) {
        const hash = await this.hash(block);
        if (this.sentHashes.has(hash)) continue;
        if (!isBlockComplete(block)) continue;
        const dangerous = containsDangerousPattern(block);
        if (dangerous) {
          this.onDangerousPattern(block, dangerous);
          this.sentHashes.add(hash);
          continue;
        }
        this.sentHashes.add(hash);
        this.onOpsReady(block, this.turn);
      }
    } catch (e) {
      console.warn('glyim-pilot: stream watcher check failed:', e);
    }
  }

  private handleStreamComplete(): void {
    if (this.completed) return;
    this.completed = true;
    const full = this.adapter.getAssistantText();
    if (full) this.onStreamComplete(full, this.turn);
    this.sentHashes.clear();
  }

  private async hash(content: string): Promise<string> {
    const data = new TextEncoder().encode(content);
    const hashBuffer = await crypto.subtle.digest('SHA-256', data);
    return Array.from(new Uint8Array(hashBuffer))
      .map(b => b.toString(16).padStart(2, '0'))
      .join('')
      .slice(0, 16);
  }
}

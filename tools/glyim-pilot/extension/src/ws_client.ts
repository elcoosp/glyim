import type { ExtensionMessage, CliMessage } from './types';
import { PROTOCOL_VERSION, validateMessageVersion } from './types';

const DEFAULT_URL = 'ws://127.0.0.1:8420';
const RECONNECT_BASE_DELAY = 1000;
const RECONNECT_MAX_DELAY = 10000;
const PING_INTERVAL = 30000;

export class WsClient {
  private ws: WebSocket | null = null;
  private url: string;
  private reconnectAttempts = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private intentionalClose = false;
  private messageHandler: ((msg: CliMessage) => void) | null = null;
  /** T201-PATCHED [EXT-8]: pending outbound messages while disconnected. */
  private queue: ExtensionMessage[] = [];
  private statusHandler: ((connected: boolean) => void) | null = null;

  constructor(url: string = DEFAULT_URL) { this.url = url; }
  onMessage(handler: (msg: CliMessage) => void): void { this.messageHandler = handler; }
  onStatusChange(handler: (connected: boolean) => void): void { this.statusHandler = handler; }
  connect(): void { this.intentionalClose = false; this.doConnect(); }
  disconnect(): void { this.intentionalClose = true; this.cleanup(); this.ws?.close(); this.ws = null; }
  send(msg: ExtensionMessage): boolean {
    // T201-PATCHED [EXT-8]: queue outbound messages when the socket is
    // not OPEN. Previously a message produced while the WS was
    // reconnecting (server restart, MV3 service-worker suspension) was
    // silently dropped — the server never learned a turn completed and
    // the AI waited forever. The queue is bounded (50 messages) and
    // flushed on `onopen`.
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
      return true;
    }
    if (this.queue.length < 50) {
      this.queue.push(msg);
    }
    return false;
  }
  get connected(): boolean { return this.ws !== null && this.ws.readyState === WebSocket.OPEN; }

  private doConnect(): void {
    try { this.ws = new WebSocket(this.url); } catch (e) { console.warn('glyim-pilot: WS creation failed:', e); this.scheduleReconnect(); return; }
    this.ws.onopen = () => {
      this.reconnectAttempts = 0;
      this.statusHandler?.(true);
      this.startPing();
      // T201-PATCHED [EXT-8]: flush queued messages produced while
      // disconnected. Sent in FIFO order.
      const pending = this.queue.splice(0);
      for (const msg of pending) {
        try { this.ws?.send(JSON.stringify(msg)); } catch (e) {
          console.warn('glyim-pilot: failed to flush queued message:', e);
          break;
        }
      }
    };
    this.ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data) as CliMessage;
        const versionError = validateMessageVersion((msg as any).v as number | undefined);
        if (versionError) console.warn(`glyim-pilot: ${versionError}`);
        this.messageHandler?.(msg);
      } catch (e) { console.warn('glyim-pilot: failed to parse WS message:', e); }
    };
    this.ws.onclose = () => { this.statusHandler?.(false); this.stopPing(); this.ws = null; if (!this.intentionalClose) this.scheduleReconnect(); };
    this.ws.onerror = (e) => { console.warn('glyim-pilot: WS error:', e); };
  }

  private scheduleReconnect(): void {
    if (this.intentionalClose) return;
    const delay = Math.min(RECONNECT_BASE_DELAY * Math.pow(2, this.reconnectAttempts), RECONNECT_MAX_DELAY);
    this.reconnectAttempts++;
    this.reconnectTimer = setTimeout(() => this.doConnect(), delay);
  }
  private startPing(): void {
    this.stopPing();
    this.pingTimer = setInterval(() => {
      if (this.ws && this.ws.readyState === WebSocket.OPEN) {
        // Send a pong message (valid ExtensionMessage) to keep the connection alive
        this.send({ type: 'pong', timestamp: Date.now(), v: PROTOCOL_VERSION });
      }
    }, 20000);
  } private stopPing(): void { if (this.pingTimer) clearInterval(this.pingTimer); }
  private cleanup(): void { if (this.reconnectTimer) clearTimeout(this.reconnectTimer); this.stopPing(); }
}

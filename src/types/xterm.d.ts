/* Type declarations for xterm.js modules */
declare module "@xterm/xterm" {
  export class Terminal {
    constructor(options?: Record<string, unknown>);
    open(container: HTMLElement): void;
    write(data: string): void;
    writeln(data: string): void;
    onData(callback: (data: string) => void): void;
    clear(): void;
    dispose(): void;
    loadAddon(addon: unknown): void;
    cols: number;
    rows: number;
  }
}

declare module "@xterm/addon-fit" {
  export class FitAddon {
    fit(): void;
    dispose(): void;
  }
}

declare module "@xterm/addon-web-links" {
  export class WebLinksAddon {
    dispose(): void;
  }
}

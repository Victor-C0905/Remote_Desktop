/** chrome-tabs 库类型声明（无官方类型） */
declare module 'chrome-tabs' {
  export default class ChromeTabs {
    constructor();
    init(el: HTMLElement): void;
    addTab(
      props: { title?: string; favicon?: string | false; id?: string },
      opts?: { animate?: boolean; background?: boolean },
    ): void;
    removeTab(tabEl: HTMLElement): void;
    updateTab(
      tabEl: HTMLElement,
      props: { title?: string; favicon?: string | false; id?: string },
    ): void;
    setCurrentTab(tabEl: HTMLElement): void;
    layoutTabs(): void;
  }
}

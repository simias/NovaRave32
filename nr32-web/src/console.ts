export function redirectConsole(elem: HTMLPreElement) {
  elem.textContent = '';

  function logToPage(type: string, ...args: unknown[]) {
    if (!elem.textContent || elem.textContent.length > 1024 * 1024) {
      elem.textContent = '';
    }

    const msg = args
      .map((arg) => (typeof arg === 'object' ? JSON.stringify(arg, null, 2) : arg))
      .join(' ');
    elem.textContent += `[${type}] ${msg}\n`;
  }

  (['log', 'info', 'warn', 'error'] as const).forEach((m) => {
    const method: keyof Console = m;
    const oldMethod = console[method];

    const label = method.toUpperCase();

    console[method] = function (...args: unknown[]) {
      oldMethod.apply(console, args);
      logToPage(label, ...args);
    };
  });

  window.addEventListener('error', (event) => {
    logToPage('EXCEPTION', event.message);
  });

  window.addEventListener('unhandledrejection', (event) => {
    logToPage(
      'REJECT',
      event.reason instanceof Error ? event.reason.message : String(event.reason),
    );
  });
}

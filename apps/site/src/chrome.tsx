export function Nav() {
  return (
    <nav className="flex h-[68px] items-center justify-between border-b">
      <a href="/" className="flex items-center gap-2.5 text-xl font-semibold tracking-tight">
        <svg width="20" height="20" viewBox="0 0 32 32" aria-hidden>
          <polygon points="16,6 26,28 6,28" fill="var(--primary)" fillOpacity="0.35" />
          <circle cx="16" cy="7" r="4" fill="var(--primary)" />
        </svg>
        lux
      </a>
      <div className="flex items-center gap-7 text-[15px]">
        <a className="text-mut transition-colors hover:text-ink" href="/#how">
          How it works
        </a>
        <a className="text-mut transition-colors hover:text-ink" href="/privacy/">
          Privacy
        </a>
      </div>
    </nav>
  );
}

export function Footer() {
  return (
    <footer className="border-t py-10 pb-14 text-[15px] text-mut">
      <div className="flex flex-wrap items-center justify-between gap-5">
        <p>© 2026 John Carmack. The opera singer, not the Doom guy.</p>
        <div className="flex gap-5">
          <a className="transition-colors hover:text-ink" href="https://github.com/johncarmack1984/lux">
            GitHub
          </a>
          <a className="transition-colors hover:text-ink" href="https://johncarmack.com">
            johncarmack.com
          </a>
          <a className="transition-colors hover:text-ink" href="https://x.com/johnmcarmack">
            @johnmcarmack
          </a>
          <a className="transition-colors hover:text-ink" href="/privacy/">
            Privacy
          </a>
        </div>
      </div>
    </footer>
  );
}

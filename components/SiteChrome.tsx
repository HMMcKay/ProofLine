import Link from "next/link";
import type { ReactNode } from "react";

export function SiteChrome({ children }: { children: ReactNode }) {
  return <div className="site-shell">
    <header className="site-header">
      <Link className="brand" href="/"><span className="brand-mark">PL</span>ProofLine</Link>
      <nav className="site-nav" aria-label="Primary navigation">
        <Link href="/my">My videos</Link>
        <Link href="/protocol">Protocol</Link>
        <Link className="record-link" href="/capture">Record now</Link>
      </nav>
    </header>
    {children}
    <footer className="footer"><span>High-assurance provenance evidence, not a guarantee of objective truth.</span><span>ProofLine protocol v2</span></footer>
  </div>;
}

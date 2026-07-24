import { useEffect, useRef, useState } from 'react';
import {
  CheckItem,
  CommandPalette,
  Panel,
  Prompt,
  TerminalFrame,
} from '../components';
import type { CommandPaletteItem } from '../components';
import lamplighterUrl from '../../design/lamplighter.png';
import demoUrl from './assets/demo.svg';
import { CopyCommand } from './CopyCommand';
import './landing.css';

const REPO_URL = 'https://github.com/itsthelore/asdecided-core';

const DEMO_ALT =
  'Recorded terminal session: brew install itsthelore/tap/rac-full, ' +
  'claude mcp add lore -- rac mcp, then rac find, rac resolve and ' +
  "rac validate run against this repository's corpus, ending in PASS";

/**
 * The animated demo recording. The SVG animates continuously, which is
 * heavy enough on the main thread to wreck LCP if it renders during
 * page load — so the img mounts only once the section nears the
 * viewport. The frame reserves the SVG's aspect ratio (no layout
 * shift), and a noscript fallback keeps the recording reachable
 * without JavaScript.
 */
function DemoRecording() {
  const frameRef = useRef<HTMLDivElement>(null);
  const [show, setShow] = useState(false);

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame || !('IntersectionObserver' in window)) {
      setShow(true);
      return;
    }
    // A threshold rather than a margin: on small viewports a sliver of
    // the frame peeks above the fold at load, and mounting then would
    // put the animation's main-thread cost inside the LCP window.
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setShow(true);
          observer.disconnect();
        }
      },
      { threshold: 0.15 },
    );
    observer.observe(frame);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={frameRef} className="demo__frame">
      {show ? (
        <img
          className="demo__recording"
          src={demoUrl}
          width={840}
          height={581}
          alt={DEMO_ALT}
        />
      ) : (
        <noscript
          dangerouslySetInnerHTML={{
            __html: `<img class="demo__recording" src="${demoUrl}" width="840" height="581" alt="${DEMO_ALT}">`,
          }}
        />
      )}
    </div>
  );
}

/** Scroll a page section into view and move focus to it. */
function goToSection(id: string) {
  const el = document.getElementById(id);
  if (!el) return;
  el.scrollIntoView({ block: 'start' });
  el.focus({ preventScroll: true });
}

// The footer palette is a navigator, not a chatbot: every item is a
// real target — a section on this page or a page that exists.
const paletteItems: CommandPaletteItem[] = [
  {
    label: 'Demo',
    hint: 'section',
    action: () => goToSection('demo'),
  },
  {
    label: 'MCP tools',
    hint: 'section',
    action: () => goToSection('tools'),
  },
  {
    label: 'Get Lore',
    hint: 'section',
    action: () => goToSection('get'),
  },
  {
    label: 'Why agents do better with Lore',
    hint: 'section',
    action: () => goToSection('why'),
  },
  {
    label: 'GitHub repository',
    hint: 'link',
    action: () => {
      window.location.href = REPO_URL;
    },
  },
  {
    label: 'Design system demo',
    hint: 'page',
    action: () => {
      window.location.href = './demo/';
    },
  },
  {
    label: 'Export viewer',
    hint: 'page',
    action: () => {
      window.location.href = './viewer/';
    },
  },
];

export function LandingApp() {
  return (
    <>
      <div className="landing">
        <TerminalFrame title="Lore — open source">
          <div className="landing__grid">
            <main className="landing__main">
              <header className="hero">
                <img
                  className="hero__mascot"
                  src={lamplighterUrl}
                  width={500}
                  height={395}
                  alt="Lore's lamplighter mascot holding a lantern"
                />
                <div className="hero__copy">
                  <h1 className="hero__title">
                    Your ADRs &amp; Decisions, Served to Your Coding Agent
                    over MCP
                  </h1>
                  <p className="hero__sub">
                    Ground every agent edit in what your team actually
                    decided.
                  </p>
                </div>
              </header>

              <section aria-labelledby="diff-heading">
                <h2 id="diff-heading" className="landing__h2">
                  What makes Lore different:
                </h2>
                <ul className="diff__list">
                  <li className="diff__item">
                    Every answer cites a decision by ID —{' '}
                    <strong className="diff__em">ADR-001</strong>, not vibes
                  </li>
                  <li className="diff__item">
                    Deterministic graph lookups —{' '}
                    <strong className="diff__em">no RAG, no embeddings</strong>,
                    no guessing
                  </li>
                  <li className="diff__item">
                    Read-only MCP server —{' '}
                    <strong className="diff__em">four tools, one command</strong>,
                    zero config
                  </li>
                </ul>
              </section>

              <section aria-labelledby="next-heading">
                <h2 id="next-heading" className="landing__h2">
                  What would you like to do next?
                </h2>
                <div className="next__list">
                  <Prompt variant="next" index={1}>
                    <a href="#demo">Watch the demo.</a>
                  </Prompt>
                  <Prompt variant="next" index={2}>
                    <a href="#tools">See the four MCP tools.</a>
                  </Prompt>
                  <Prompt variant="next" index={3}>
                    <a href="#get">Get Lore.</a>
                  </Prompt>
                </div>
              </section>

              <section
                id="demo"
                className="landing__section"
                tabIndex={-1}
                aria-labelledby="demo-heading"
              >
                <h2 id="demo-heading" className="landing__h2">
                  Demo — zero to a validated corpus
                </h2>
                <DemoRecording />
                <p className="demo__caption">
                  Real session recorded against this repository's own corpus
                  — every command shown actually ran. Reproducible via
                  scripts/record-demo.sh.
                </p>
              </section>

              <section
                id="get"
                className="landing__section"
                tabIndex={-1}
                aria-labelledby="get-heading"
              >
                <h2 id="get-heading" className="landing__h2">
                  Get Lore
                </h2>
                <div className="get__steps">
                  <p className="get__step">
                    <span className="get__lead">Install:</span>{' '}
                    <CopyCommand command="brew install itsthelore/tap/rac-full" />
                  </p>
                  <p className="get__step">
                    <span className="get__lead">
                      Connect your agent (Claude Code, from your repo root):
                    </span>{' '}
                    <CopyCommand command="claude mcp add lore -- rac mcp" />
                  </p>
                </div>
                <p className="get__note">
                  Lore is built on RAC — Requirements as Code — the
                  open-source engine underneath; for now the package, CLI
                  and MCP server ship under the rac name. Source:{' '}
                  <a href={REPO_URL}>github.com/itsthelore/asdecided-core</a>
                </p>
              </section>
            </main>

            <aside className="landing__rail" aria-label="Lore at a glance">
              <h2 className="rail__heading">
                See the same prompt run twice — with and without the lore
              </h2>
              <p className="rail__note">
                Comparison recording pending — see the{' '}
                <a href="#demo">toolchain demo</a> for now.
              </p>

              <section
                id="tools"
                className="landing__section"
                tabIndex={-1}
                aria-labelledby="tools-heading"
              >
                <h3 id="tools-heading" className="rail__label">
                  MCP tools:
                </h3>
                <div className="rail__tools">
                  <Prompt command="get_summary" description="repo decision map" />
                  <Prompt
                    command="search_artifacts"
                    description="find the relevant decision"
                  />
                  <Prompt command="get_artifact" description="full record, by ID" />
                  <Prompt command="get_related" description="walk the graph" />
                </div>
                <h3 className="rail__label">and in CI:</h3>
                <Prompt
                  command="rac validate"
                  description="gate the graph on every push"
                />
              </section>

              <section
                id="why"
                className="landing__section"
                tabIndex={-1}
                aria-label="Why agents do better with Lore"
              >
                <Panel title="Why agents do better with Lore">
                  <CheckItem>
                    Citations by ID — decisions land in the diff
                  </CheckItem>
                  <CheckItem>
                    Typed Markdown + YAML, versioned with your code
                  </CheckItem>
                  <CheckItem>
                    One command:{' '}
                    <CopyCommand command="claude mcp add lore -- rac mcp" />
                  </CheckItem>
                </Panel>
              </section>
            </aside>
          </div>
        </TerminalFrame>
      </div>

      <footer aria-label="Page navigation">
        <CommandPalette
          items={paletteItems}
          prompt="lore-$"
          fixed
          ariaLabel="Navigate the page"
        />
      </footer>
    </>
  );
}

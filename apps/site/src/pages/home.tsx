import { buttonVariants } from "@lux/ui";
import { Nav, Footer } from "../chrome";
import { LiveDesk } from "../live-desk";

const DOWNLOAD_URL = "https://github.com/johncarmack1984/lux/releases/latest";
const REPO_URL = "https://github.com/johncarmack1984/lux";

export function Home() {
  return (
    <div className="mx-auto max-w-[1120px] px-6">
      <Nav />

      <header className="rise grid items-center gap-14 py-16 pb-20 max-md:grid-cols-1 md:grid-cols-[1.05fr_0.95fr]">
        <div>
          <h1 className="text-[clamp(38px,5.2vw,58px)] font-bold leading-[1.04] tracking-[-0.03em]">
            Slide a fader.
            <br />
            Light the room.
          </h1>
          <p className="mt-5 max-w-[44ch] text-[19px] text-mut">
            lux is a free, open-source DMX controller for macOS. A full
            512-channel universe, your fixtures, no subscription.
          </p>
          <div className="mt-8 flex flex-wrap gap-3.5">
            <a className={buttonVariants({ variant: "primary" })} href={DOWNLOAD_URL}>
              Download for macOS
            </a>
            <a className={buttonVariants({ variant: "ghost" })} href={REPO_URL}>
              GitHub
            </a>
          </div>
        </div>
        <LiveDesk />
      </header>

      <section className="border-t py-16">
        <h2 className="text-[clamp(26px,3.4vw,36px)] font-semibold tracking-[-0.02em]">
          The actual app.
        </h2>
        <p className="mt-3 max-w-[62ch] text-mut">
          No mockups. This is lux on a Mac: one fixture patched, amber up,
          dimmer low.
        </p>
        <figure className="mt-8">
          <img
            src="/app.png"
            width="1072"
            height="1176"
            loading="lazy"
            alt="The lux app on macOS showing a Default Fixture with six channel sliders: Red, Green, Blue, Amber, White, and Dimmer"
            className="mx-auto w-full max-w-[720px] rounded-(--radius) border"
          />
          <figcaption className="mt-3 text-center text-sm text-mut">
            Current build. The light really did that.
          </figcaption>
        </figure>
      </section>

      <section id="how" className="border-t py-16">
        <h2 className="text-[clamp(26px,3.4vw,36px)] font-semibold tracking-[-0.02em]">
          How it works.
        </h2>
        <div className="mt-9 grid grid-cols-2 gap-4 max-md:grid-cols-1">
          <div className="relative overflow-hidden rounded-(--radius) border bg-surface p-7 md:col-span-2">
            <div className="grid-bg" />
            <h3 className="relative text-[21px] font-semibold tracking-[-0.01em]">
              The whole universe.
            </h3>
            <p className="relative mt-2 max-w-[52ch] text-mut">
              512 channels, a fader for every one. Patch fixtures over the top
              when you'd rather see names than numbers.
            </p>
          </div>
          <div className="rounded-(--radius) border bg-surface p-7">
            <h3 className="text-[21px] font-semibold tracking-[-0.01em]">Your fixtures.</h3>
            <p className="mt-2 text-mut">
              Built-in presets, or define the channels yourself. Color mixing
              that knows what the amber channel is for.
            </p>
          </div>
          <div className="rounded-(--radius) border bg-surface p-7">
            <h3 className="text-[21px] font-semibold tracking-[-0.01em]">Three ways out.</h3>
            <ul className="mono mt-3 space-y-2 text-sm">
              <li>
                <span className="text-accent">→ </span>Enttec Open DMX USB
              </li>
              <li>
                <span className="text-accent">→ </span>sACN (E1.31) over the network
              </li>
              <li>
                <span className="text-accent">→ </span>Art-Net node discovery
              </li>
            </ul>
            <p className="mt-3 text-mut">Network nodes show up on their own.</p>
          </div>
          <div className="rounded-(--radius) border bg-surface p-7 md:col-span-2">
            <h3 className="text-[21px] font-semibold tracking-[-0.01em]">
              Setups that follow you.
            </h3>
            <p className="mt-2 max-w-[60ch] text-mut">
              Home, church, work: each one a fixture patch bound to a universe.
              Sign in and they sync between your machines. Don't, and
              everything stays on this one.
            </p>
          </div>
        </div>
      </section>

      <section className="border-t py-16">
        <p className="max-w-[36ch] text-[clamp(21px,2.6vw,27px)] leading-[1.45] tracking-[-0.01em]">
          lux won't run your timecode, your pixel maps, or your forty-universe
          festival. It runs <span className="text-accent">one universe, really well</span>,
          from hardware you already own. A stage. A sanctuary. A garage show.
          Your living room, if it's that kind of living room.
        </p>
      </section>

      <section className="border-t py-16">
        <p className="text-[clamp(30px,4vw,44px)] font-bold tracking-[-0.02em]">
          Free. GPLv3.
        </p>
        <p className="mt-3.5 max-w-[52ch] text-mut">
          No subscription, no tiers. The code is public; bring issues, bring
          PRs. macOS today, signed and notarized, updates itself. iPhone next.
        </p>
        <a
          className={buttonVariants({ variant: "primary" }) + " mt-7"}
          href={DOWNLOAD_URL}
        >
          Download for macOS
        </a>
      </section>

      <Footer />
    </div>
  );
}

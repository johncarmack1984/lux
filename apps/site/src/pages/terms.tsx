import { Nav, Footer } from "../chrome";

export function Terms() {
  return (
    <div className="mx-auto max-w-[1120px] px-6">
      <Nav />

      <article className="mx-auto max-w-[68ch] py-14 pb-24 [&_h2]:mt-10 [&_h2]:mb-2.5 [&_h2]:text-2xl [&_h2]:font-semibold [&_li]:my-1.5 [&_li]:text-[#c6ccd4] [&_p]:my-3 [&_p]:text-[#c6ccd4] [&_strong]:text-ink [&_ul]:my-3 [&_ul]:ml-6 [&_ul]:list-disc">
        <h1 className="text-[clamp(32px,4vw,44px)] font-bold tracking-[-0.02em]">
          Terms
        </h1>
        <p className="mt-1.5 !text-[15px] !text-mut">
          Effective August 10, 2026 · applies to lux for macOS and iOS, the lux
          account, and this site
        </p>

        <h2>The short version</h2>
        <p>
          The app is free and open source, and it works without an account. An
          account exists only for the things that cross a boundary — another
          device, another person, another company's service. Two sections below
          are worth reading twice:{" "}
          <strong>lighting control</strong> and{" "}
          <strong>Planning Center</strong>. They are the ones that matter when
          something goes wrong during a service.
        </p>

        <h2>Who you're agreeing with</h2>
        <p>
          lux is made and run by John Carmack —{" "}
          <a className="text-accent underline underline-offset-3" href="mailto:johncarmack@me.com">
            johncarmack@me.com
          </a>
          . "We" means that. "You" means you, and if you're agreeing on behalf
          of a church or an organization, it means that organization and you're
          confirming you may bind it.
        </p>

        <h2>The app is free, and it's yours</h2>
        <p>
          The lux desktop and mobile apps are free and open source under the
          GNU General Public License v3.0. That license governs the software
          itself, and{" "}
          <strong>nothing in these terms takes away a right it grants you</strong>.
          The full 512-channel desk, all three outputs, custom fixtures,
          scenes, and manual control work with no account and no payment.
        </p>

        <h2>There's nothing to buy</h2>
        <p>
          lux has no paid plan, no subscription, and no in-app purchase. If
          that ever changes, the terms for it will be published here before
          anyone is asked to pay, and the offline app will stay free.
        </p>

        <h2>Accounts</h2>
        <p>
          You need an account only for the features that leave your machine:
          syncing setups between your devices, sharing control with someone
          else, connecting Planning Center. You're responsible for what happens
          under yours. Tell us at the address above if you think someone else
          has it.
        </p>
        <p>
          Don't hand out your password to share a rig — share the setup
          instead. That's a thing lux does properly, and it's revocable.
        </p>

        <h2>Sharing control</h2>
        <p>
          Sharing a setup gives another lux account real control of real lights
          on your rig. Give it only to people you trust, and know what it costs
          to take back:
        </p>
        <ul>
          <li>
            An invite is single-use and expires on its own. Either side can end
            a grant at any time — revoking and leaving are the same act named
            from opposite ends, and neither party needs the other's
            cooperation.
          </li>
          <li>
            <strong>Revoking is not instant for a guest who is already
            connected.</strong> Their access is checked when they connect, and
            their connection re-checks about once an hour, so someone already
            at your desk may keep it until then. If you need them off the rig
            right now, pull the setup off the network or close the app.
          </li>
        </ul>

        <h2>Planning Center</h2>
        <p>
          If you connect Planning Center, you're confirming you're allowed to
          do that for your organization.
        </p>
        <ul>
          <li>
            <strong>lux only reads.</strong> It never creates, edits, or
            deletes anything in Planning Center, and it never advances your
            service. That's a property of how the software is built, not only a
            promise — the Privacy Policy says exactly how.
          </li>
          <li>
            lux reads your service types and, for the one you pick, the next
            plan and its items. Precisely what that means is in the{" "}
            <a className="text-accent underline underline-offset-3" href="/privacy/">
              Privacy Policy
            </a>
            .
          </li>
          <li>
            Planning Center is your provider, not ours. Your relationship with
            them, and their availability, are between you and them. If they're
            down, change their API, or end your access, lux's plan features
            stop — and, per the next section, your lights do not.
          </li>
          <li>
            Disconnect at any time, from lux or from Planning Center. Either
            one ends it.
          </li>
        </ul>

        <h2>Lighting control — read this one twice</h2>
        <p>
          <strong>lux is a tool, and you are the operator.</strong>
        </p>
        <ul>
          <li>
            <strong>Local control is authoritative and doesn't depend on
            us.</strong> Live DMX output goes from your machine to your
            hardware. If our servers are down, your internet is out, or
            Planning Center is unreachable, your lights hold and manual control
            keeps working. That's how lux is built, and it's the reason the
            cloud features are allowed to exist at all.
          </li>
          <li>
            The plan view shows you your running order. It does not touch your
            lights on its own — you're the one pressing Go — and nothing in it
            sits in the path between a fader and a fixture.
          </li>
          <li>
            <strong>Keep a person who can take over.</strong> For any service
            that matters, have someone at the desk who can run it by hand, and
            make sure they know how.
          </li>
          <li>
            <strong>Do not use lux where a lighting failure could hurt
            someone</strong>, or where a specific light must be guaranteed on
            or off for safety. It is not certified for life-safety, emergency
            lighting, or anything with that kind of consequence. Strobing and
            rapid intensity changes can trigger photosensitive seizures; what
            you put in front of an audience is your call and your
            responsibility.
          </li>
        </ul>

        <h2>Acceptable use</h2>
        <p>
          Don't use lux to break the law, don't try to get into other people's
          accounts or setups, and don't attack the service.
        </p>

        <h2>Availability</h2>
        <p>
          We try to keep the cloud features up, but there's no promised uptime
          and no SLA. We may change or retire a cloud feature with reasonable
          notice. <strong>The offline app is the guarantee; the servers are the
          convenience.</strong>
        </p>

        <h2>Your content</h2>
        <p>
          Your setups, your scenes, and your plan data are yours. We claim no
          ownership of them and no license beyond storing and transmitting them
          to give you the features you turned on. Deleting your account removes
          them from our servers — the{" "}
          <a className="text-accent underline underline-offset-3" href="/privacy/">
            Privacy Policy
          </a>{" "}
          says what that reaches, and what you should end yourself first.
        </p>

        <h2>Warranties and liability</h2>
        <p>
          The hosted service is provided "as is", without warranties beyond
          those the law doesn't let us disclaim. (The software itself carries
          the GPL's own warranty disclaimer; this covers the servers.) To the
          extent the law allows, we're not liable for indirect or consequential
          losses — a missed cue, a service that didn't look the way you wanted,
          lost volunteer time. Nothing here limits liability for death or
          personal injury caused by negligence, or for anything else that can't
          be limited.
        </p>

        <h2>Ending it</h2>
        <p>
          Stop using it whenever you like; delete the account from the account
          menu. We may suspend or end an account that breaks these terms, and
          we'll say why unless we're not allowed to. When it ends, the cloud
          features stop and the free app carries on.
        </p>

        <h2>Changes</h2>
        <p>
          If these terms change, the new text lands on this page with a new
          date at the top. If a change is material and you have an account,
          we'll tell you before it takes effect.
        </p>

        <h2>Contact</h2>
        <p>
          Questions:{" "}
          <a className="text-accent underline underline-offset-3" href="mailto:johncarmack@me.com">
            johncarmack@me.com
          </a>
        </p>
      </article>

      <Footer />
    </div>
  );
}

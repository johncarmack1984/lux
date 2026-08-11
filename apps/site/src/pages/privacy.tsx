import { Nav, Footer } from "../chrome";

export function Privacy() {
  return (
    <div className="mx-auto max-w-[1120px] px-6">
      <Nav />

      <article className="mx-auto max-w-[68ch] py-14 pb-24 [&_h2]:mt-10 [&_h2]:mb-2.5 [&_h2]:text-2xl [&_h2]:font-semibold [&_h3]:mt-6 [&_h3]:mb-1.5 [&_h3]:text-[17px] [&_h3]:font-semibold [&_li]:my-1.5 [&_li]:text-[#c6ccd4] [&_p]:my-3 [&_p]:text-[#c6ccd4] [&_strong]:text-ink [&_ul]:my-3 [&_ul]:ml-6 [&_ul]:list-disc">
        <h1 className="text-[clamp(32px,4vw,44px)] font-bold tracking-[-0.02em]">
          Privacy
        </h1>
        <p className="mt-1.5 !text-[15px] !text-mut">
          Effective August 10, 2026 · applies to lux for macOS and iOS
        </p>

        <h2>The short version</h2>
        <p>
          lux doesn't want your data. Everything lives on your machine unless
          you create an account, and the account exists to sync your lighting
          setups between your devices. That's the whole business model: there
          isn't one.
        </p>

        <h2>On your device</h2>
        <p>
          Your setups (their names, universe numbers, fixture definitions, and
          the scenes you save on them) are stored in a local file on your
          computer, next to a small file of app preferences. Those files never
          leave your device unless you sign in.
        </p>
        <p>
          lux has <strong>no analytics, no crash reporting, no tracking, and no
          third-party advertising SDKs</strong>. There is no telemetry. I'd have to
          build telemetry, and I don't want to.
        </p>

        <h2>If you create an account</h2>
        <p>lux stores these things:</p>
        <ul>
          <li>
            <strong>Your email address</strong> — your sign-in identity, held
            in AWS Cognito. With Sign in with Apple that's whichever address
            Apple hands over, including the private relay address if you chose
            to hide yours.
          </li>
          <li>
            <strong>Your setups</strong> — the same names, universe numbers,
            fixture definitions, and saved scenes from your device, held in AWS
            DynamoDB (US West).
          </li>
          <li>
            <strong>Your app preferences</strong> — one small settings record
            that syncs alongside the setups. Today it holds which way the
            faders run.
          </li>
          <li>
            <strong>Sharing, if you share a setup</strong> — who you gave
            control of which setup, the label you gave them in your own list,
            and the matching record on their side so the setup shows up for
            them. An invite nobody has claimed yet is stored as a hash of the
            code rather than the code itself, and expires on its own.
          </li>
          <li>
            <strong>A Sign in with Apple link, if you use one</strong> —
            Apple's stable identifier for you, tied to your lux account, plus
            the token lux needs to tell Apple to end the link when you delete
            the account.
          </li>
          <li>
            <strong>Paired devices, if you run lux-node</strong> — the device's
            id, the name it reported, and the setup and universe it was paired
            to. The short-lived record that carries out the pairing itself is
            keyed by a hash of the code you type, notes the public IP the
            approval came from, and deletes itself when it expires.
          </li>
          <li>
            <strong>A Planning Center connection, if you make one</strong> —
            the next section is entirely about that.
          </li>
        </ul>
        <p>
          That's the list. It is longer than it was in July, because the
          software grew; when lux stores something new, this page says so.
        </p>
        <p>
          Your password never leaves your device: password sign-in uses SRP, a
          zero-knowledge proof, so lux's servers verify you know the password
          without ever seeing it. Sign in with Apple has no password for anyone
          to see. Session tokens are kept in your operating system's keychain.
        </p>

        <h2>If you connect Planning Center</h2>
        <p>
          Connecting is something you do, from the app, one lux account at a
          time — and it finishes on Planning Center's own approval screen. lux
          can only ever see what the person who approves it can see.
        </p>

        <h3>What lux reads</h3>
        <p>
          Your organization's name and id, your service types, and — for the
          service type you pick — the next plan and its items: each item's
          title, its type (song, header, media, or one your church made up),
          its planned length, its place in the running order, and for songs
          Planning Center's id for the song. That's the list.
        </p>

        <h3>What lux never does</h3>
        <p>
          <strong>lux never writes to Planning Center.</strong> It cannot
          create, edit, or delete a plan, and it cannot advance your service.
          The permission Planning Center calls "services" has no read-only
          version, so that isn't something the grant enforces — it's built into
          lux, whose Planning Center client issues read requests and has no
          method that sends anything else.
        </p>
        <p>
          lux asks for "services" and nothing else: not your people, giving,
          check-ins, groups, registrations, or calendar. It doesn't read
          attachments, arrangements, keys, item notes, or anything about who is
          scheduled.
        </p>

        <h3>What lux keeps</h3>
        <p>
          When you approve, Planning Center hands lux a token. It is stored on
          lux's servers — with your organization's name and id and the date you
          connected — and it is never sent to the app on your desk or your
          phone. That is the reason this part runs on a server at all. The plan
          itself is read when you ask for it and passed straight through to the
          app: <strong>lux does not store your plans, your items, or your song
          list</strong>. While you're connecting, a single-use record holds the
          request for fifteen minutes and then expires by itself.
        </p>

        <h3>Reconnecting</h3>
        <p>
          Planning Center's authorization runs 90 days from the last time it
          was renewed. lux renews it quietly while you keep using it, and says
          something a week before the ceiling, so you reconnect on a weekday
          instead of finding out on a Sunday. Reconnecting is the same approval
          screen again.
        </p>

        <h3>Turning it off</h3>
        <p>
          <strong>Disconnect</strong>, in the Plan view, hands the token back
          to Planning Center and then deletes it here — a delete, not a flag.
          You can also revoke lux in Planning Center's own settings, which
          stops it immediately; lux's next read then fails and asks you to
          connect again. Deleting your lux account does the same thing
          Disconnect does, so you don't have to remember to do it first.
        </p>

        <h3>Whose data it is</h3>
        <p>
          Your plans are your church's. Planning Center is your provider, not
          lux's. lux reads them to turn a plan into lighting cues and for
          nothing else — not analyzed, not aggregated, not used to train
          anything.
        </p>

        <h2>Network calls lux makes</h2>
        <ul>
          <li>
            <strong>Update checks</strong> query GitHub Releases for a newer
            version. GitHub sees the standard metadata of any web request, like
            your IP address.
          </li>
          <li>
            <strong>Sync</strong>, when signed in, talks to lux's own API on
            AWS.
          </li>
          <li>
            <strong>Change notifications</strong>, when signed in, ride a
            connection to AWS IoT so your other devices refresh promptly. The
            messages say "something changed" and nothing else.
          </li>
          <li>
            <strong>Shared control</strong>, when you share a setup or someone
            shares one with you, rides that same AWS IoT connection. What
            crosses it is the desk itself: the layout of the setup being
            shared, and fader values in both directions.
          </li>
          <li>
            <strong>Planning Center</strong>, when you connect it, goes through
            lux's own bridge on AWS. The app never holds a Planning Center
            credential and never contacts Planning Center directly.
          </li>
          <li>
            <strong>Discord remote control</strong> exists only if you build
            and configure it yourself from the source; nothing is set up by
            default.
          </li>
        </ul>

        <h2>Server logs</h2>
        <p>
          The services behind sign-in, sync, and the Planning Center bridge
          write short operational logs — enough to see that something failed
          and roughly where. They don't carry your password, your tokens, or
          the contents of your plans, and AWS deletes them after 14 days.
        </p>

        <h2>What lux never does</h2>
        <p>
          lux does not sell data, show ads, run analytics, fingerprint your
          device, or share anything with data brokers. No exceptions.
        </p>

        <h2>Deleting your account</h2>
        <p>
          Account menu → Delete account. That permanently deletes your synced
          setups and scenes, your settings record, and your sharing grants from
          lux's servers, tells Apple to end the Sign in with Apple link if you
          made one, ends a Planning Center connection if you have one, drops
          the record of any paired lux-node, and removes your sign-in, right
          then. Removing the sign-in is also what ends a paired lux-node's
          access, since its credentials are your account's.
        </p>
        <p>
          The Planning Center part reaches another company's system, so here is
          how it goes: lux tells Planning Center to end the authorization, then
          deletes the token it was holding. If Planning Center can't be reached
          right then, your account is still deleted and the token is still
          dropped — the authorization can survive on their side, and you can
          clear it under lux's entry in your Planning Center settings.
        </p>
        <p>
          The local files on your devices stay yours; delete the app and its
          configuration folder if you want those gone too.
        </p>

        <h2>Changes</h2>
        <p>
          If this policy changes, the new text lands on this page with a new
          date at the top.
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

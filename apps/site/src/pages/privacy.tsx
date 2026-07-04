import { Nav, Footer } from "../chrome";

export function Privacy() {
  return (
    <div className="mx-auto max-w-[1120px] px-6">
      <Nav />

      <article className="mx-auto max-w-[68ch] py-14 pb-24 [&_h2]:mt-10 [&_h2]:mb-2.5 [&_h2]:text-2xl [&_h2]:font-semibold [&_li]:my-1.5 [&_li]:text-[#c6ccd4] [&_p]:my-3 [&_p]:text-[#c6ccd4] [&_strong]:text-ink [&_ul]:my-3 [&_ul]:ml-6 [&_ul]:list-disc">
        <h1 className="text-[clamp(32px,4vw,44px)] font-bold tracking-[-0.02em]">
          Privacy
        </h1>
        <p className="mt-1.5 !text-[15px] !text-mut">
          Effective July 3, 2026 · applies to lux for macOS and iOS
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
          Your setups (their names, universe numbers, and fixture definitions)
          are stored in a local file on your computer. That file never leaves
          your device unless you sign in.
        </p>
        <p>
          lux has <strong>no analytics, no crash reporting, no tracking, and no
          third-party advertising SDKs</strong>. There is no telemetry. I'd have to
          build telemetry, and I don't want to.
        </p>

        <h2>If you create an account</h2>
        <p>lux stores exactly two things:</p>
        <ul>
          <li>
            <strong>Your email address</strong> — your sign-in identity, held
            in AWS Cognito.
          </li>
          <li>
            <strong>Your setups</strong> — the same names, universe numbers,
            and fixture definitions from your device, held in AWS DynamoDB
            (US West).
          </li>
        </ul>
        <p>That's the entire list.</p>
        <p>
          Your password never leaves your device: sign-in uses SRP, a
          zero-knowledge proof, so lux's servers verify you know the password
          without ever seeing it. Session tokens are kept in your operating
          system's keychain.
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
            <strong>Discord remote control</strong> exists only if you build
            and configure it yourself from the source; nothing is set up by
            default.
          </li>
        </ul>

        <h2>What lux never does</h2>
        <p>
          lux does not sell data, show ads, run analytics, fingerprint your
          device, or share anything with data brokers. No exceptions.
        </p>

        <h2>Deleting your account</h2>
        <p>
          Account menu → Delete account. That permanently deletes your synced
          setups from lux's servers and removes your sign-in, right then. The
          local files on your devices stay yours; delete the app and its
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

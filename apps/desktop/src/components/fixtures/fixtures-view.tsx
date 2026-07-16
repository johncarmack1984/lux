import useFixtures from "@/hooks/useFixtures";
import useBuffer from "@/hooks/useBuffer";
import useSettings from "@/hooks/useSettings";
import FixtureCard from "./fixture-card";
import NewFixture from "./new-fixture";

export default function FixturesView() {
  const fixtures = useFixtures();
  const buffer = useBuffer();
  // Read once here and pass down, so N cards don't each subscribe; like the
  // desk, wait for the read so the stored layout is the first one painted.
  const settings = useSettings();
  const count = fixtures?.length ?? 0;

  return (
    <div className="flex w-full max-w-2xl flex-col gap-5 px-4 pb-16 pt-2">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          {fixtures === null
            ? "Loading"
            : `${count} ${count === 1 ? "fixture" : "fixtures"} patched`}
        </p>
        <NewFixture fixtures={fixtures ?? []} />
      </div>

      {fixtures !== null &&
        settings !== null &&
        (fixtures.length === 0 ? (
          <div className="rounded-xl border border-dashed py-16 text-center text-sm text-muted-foreground">
            No fixtures patched yet. Add one to get started.
          </div>
        ) : (
          // Vertical faders make the whole view a console: cards sit side by
          // side at content width and the bank scrolls sideways.
          <div
            className={
              (settings.sliderOrientation ?? "vertical") === "vertical"
                ? "flex gap-4 overflow-x-auto pb-2"
                : "flex flex-col gap-4"
            }
          >
            {[...fixtures]
              .sort((a, b) => a.address - b.address)
              .map((fixture) => (
                <FixtureCard
                  key={fixture.id}
                  fixture={fixture}
                  buffer={buffer}
                  vertical={
                    (settings.sliderOrientation ?? "vertical") === "vertical"
                  }
                />
              ))}
          </div>
        ))}
    </div>
  );
}

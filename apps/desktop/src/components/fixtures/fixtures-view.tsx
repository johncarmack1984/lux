import useFixtures from "@/hooks/useFixtures";
import useBuffer from "@/hooks/useBuffer";
import FixtureCard from "./fixture-card";
import NewFixture from "./new-fixture";

export default function FixturesView() {
  const fixtures = useFixtures();
  const buffer = useBuffer();
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
        (fixtures.length === 0 ? (
          <div className="rounded-xl border border-dashed py-16 text-center text-sm text-muted-foreground">
            No fixtures patched yet. Add one to get started.
          </div>
        ) : (
          <div className="flex flex-col gap-4">
            {[...fixtures]
              .sort((a, b) => a.address - b.address)
              .map((fixture) => (
                <FixtureCard key={fixture.id} fixture={fixture} buffer={buffer} />
              ))}
          </div>
        ))}
    </div>
  );
}

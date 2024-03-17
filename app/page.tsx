"use client";
import Fixture from "@/components/fixture";
import Greeting from "@/components/greeting";
import ButtonRow from "@/components/button-row";

export default function Home() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-between p-24">
      <Greeting />
      <ButtonRow />
      <Fixture label="Brightness" channel={6} />
    </main>
  );
}

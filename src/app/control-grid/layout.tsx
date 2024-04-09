import type { Metadata } from "next";
import ColorPicker from "@/components/color-picker/picker";
import Greeting from "@/components/greeting";
import ButtonRow from "@/components/button-row";

export const metadata: Metadata = {
  title: "Lux.app Control Grid",
  description: "Built with Tauri and Next.js",
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col max-w-3xl w-full mx-auto items-center justify-start">
      <ColorPicker className="absolute top-3 right-5" />
      <Greeting />
      <ButtonRow />
      {children}
    </div>
  );
}

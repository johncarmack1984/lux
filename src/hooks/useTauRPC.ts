import { createTauRPCProxy } from "@/app/bindings";
import { useEffect, useState } from "react";

type TauRPC = Awaited<ReturnType<typeof createTauRPCProxy>>;

const useTauRPC = () => {
  const [taurpc, setTauRPC] = useState<TauRPC | null>(null);

  useEffect(() => {
    const init = async () => {
      // const taurpc = await createTauRPCProxy();
      // setTauRPC(taurpc);
    };
    init();
  }, []);

  return taurpc;
};

export default useTauRPC;

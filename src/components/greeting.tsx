import Image from "next/image";

function Greeting() {
  return (
    <h1 className=" mt-3 leading-7 transition-all text-xl sm:text-2xl md:text-3xl whitespace-normal flex-col sm:flex-row lg:text-4xl font-bold flex gap-3 items-center">
      Welcome
      <Image
        className="inline w-[57px] h-[62px]"
        src="/lux.svg"
        alt="Vercel Logo"
        width={57}
        height={62}
      />
      to Lux
    </h1>
  );
}

export default Greeting;

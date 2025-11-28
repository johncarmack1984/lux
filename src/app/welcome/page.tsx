import { Button } from "@/components/ui/button";

export default function Component() {
  return (
    <div className="max-w-sm w-full space-y-8">
      <div>
        <h2 className="text-center text-3xl font-extrabold text-gray-900 dark:text-gray-100">
          Welcome to Lux
        </h2>
        <p className="mt-2 text-center text-sm text-gray-600 dark:text-gray-400">
          Control your lights in a smart and efficient way.
        </p>
      </div>
      <div className="flex justify-center items-center">
        <Button
          className="group relative w-full flex justify-center py-2 px-4 border border-transparent text-sm leading-5 font-medium rounded-md text-white bg-indigo-600 hover:bg-indigo-500 focus:outline-none focus:border-indigo-700 focus:shadow-outline-indigo active:bg-indigo-700 transition duration-150 ease-in-out"
          type="submit"
        >
          Connect to Devices
          <ArrowRightIcon className="ml-2 h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}

function ArrowRightIcon(props: React.SVGAttributes<SVGElement>) {
  return (
    <svg
      {...props}
      xmlns="http://www.w3.org/2000/svg"
      width="24"
      height="24"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M5 12h14" />
      <path d="m12 5 7 7-7 7" />
    </svg>
  );
}

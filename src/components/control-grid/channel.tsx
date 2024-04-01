// "use client";

// import { Slider } from "@/components/ui/slider";
// import { Button } from "@/components/ui/button";
// import { TableCell, TableRow } from "@/components/ui/table";
// import { debug } from "@tauri-apps/plugin-log";
// import type { ChannelProps, LuxChannel } from "@/global";
// import { cn, lightColorVariants } from "@/lib/utils";
// import { Input } from "../ui/input";
// import { useState } from "react";
// import { z } from "zod";
// import { zodResolver } from "@hookform/resolvers/zod";
// import { useForm } from "react-hook-form";
// import { Form, FormControl, FormField, FormItem } from "@/components/ui/form";
// import { setChannelValue, setChannelMetadata } from "@/app/actions";

// const formSchema = z.object({
//   id: z.string(),
//   label: z.string(),
//   label_color: z.enum(["Red", "Green", "Blue", "Amber", "White", "Brightness"]),
//   channel_number: z.number(),
//   disabled: z.boolean(),
// });

// function Channel({
//   id,
//   label,
//   label_color,
//   channel_number,
//   disabled,
//   value,
// }: ChannelProps) {
//   const channelNumber = channel_number;
//   const [channel, setChannel] = useState<LuxChannel>({
//     id,
//     disabled,
//     label,
//     label_color,
//     channel_number,
//   });

//   const form = useForm<z.infer<typeof formSchema>>({
//     resolver: zodResolver(formSchema),
//     defaultValues: { ...channel },
//   });

//   if (typeof value !== "number") return null;
//   if (!form) return null;

//   const handleValueChange = async (newValue: number[]) => {
//     setChannelValue({ channelNumber, value: newValue[0] });
//   };

//   const toggle = () => {
//     debug(`togggle ${channelNumber}`);
//     setChannelValue({ channelNumber, value: value > 0 ? 0 : 255 });
//   };

//   async function onSubmit(newMetadata: z.infer<typeof formSchema>) {
//     await setChannelMetadata({
//       channelId: id,
//       newMetadata,
//     });
//   }

//   return (
//     <TableRow key={`${label}-${label_color}-${channelNumber}`}>
//       <Form {...form}>
//         <TableCell className="w-5">
//           <div className={cn(lightColorVariants({ label_color }))}>
//             {channelNumber}
//           </div>
//         </TableCell>
//         <TableCell className=" min-w-40">
//           <form onSubmit={form.handleSubmit(onSubmit)}>
//             <FormField
//               control={form.control}
//               name="label"
//               render={({ field }) => (
//                 <FormItem>
//                   <FormControl>
//                     <Input
//                       className="bg-transparent border-transparent"
//                       {...field}
//                     />
//                   </FormControl>
//                 </FormItem>
//               )}
//             />
//           </form>
//         </TableCell>
//         <TableCell className="w-14">
//           <Button onClick={toggle} variant="outline" size="sm">
//             {value.toString().padStart(3, "0")}
//           </Button>
//         </TableCell>
//         <TableCell className="w-full">
//           <Slider
//             id={id}
//             value={[value]}
//             onValueChange={handleValueChange}
//             max={255}
//             step={1}
//           />
//         </TableCell>
//       </Form>
//     </TableRow>
//   );
// }

// export { Channel, type ChannelProps };

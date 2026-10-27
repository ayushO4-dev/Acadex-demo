export { cn } from "cn"

/** Academic years are stored by their first year: 2025 → "2025–26". */
export function academicYear(year: number): string {
  return `${year}–${String((year + 1) % 100).padStart(2, "0")}`;
}

const RUPEES = new Intl.NumberFormat("en-IN", { style: "currency", currency: "INR", maximumFractionDigits: 0 });
const RUPEES_AND_PAISE = new Intl.NumberFormat("en-IN", { style: "currency", currency: "INR", minimumFractionDigits: 2 });

/** 6250000 → "₹62,500"; paise appear only when there are some. */
export function formatPaise(paise: number): string {
  return (paise % 100 === 0 ? RUPEES : RUPEES_AND_PAISE).format(paise / 100);
}

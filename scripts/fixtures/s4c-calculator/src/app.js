import { calculate, formatResult } from "./calc.js";

const a = document.querySelector("#a");
const b = document.querySelector("#b");
const operation = document.querySelector("#operation");
const result = document.querySelector("#result");

document.querySelector("#calculate").addEventListener("click", () => {
  try {
    result.textContent = formatResult(
      calculate(Number(a.value), Number(b.value), operation.value),
    );
  } catch (error) {
    result.textContent = String(error.message);
  }
});

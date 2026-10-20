/** Add two numbers. */
function add(a, b) {
  return a + b;
}

function compute(a, b, op) {
  return add(a, b);
}

const aInput = document.querySelector("#a");
const bInput = document.querySelector("#b");
const opInput = document.querySelector("#op");
const result = document.querySelector("#result");

document.querySelector("#calc").addEventListener("click", () => {
  result.textContent = String(
    compute(Number(aInput.value), Number(bInput.value), opInput.value),
  );
});


/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      fontFamily: {
        sans: ['DM Sans Variable', 'sans-serif'],
        display: ['Manrope Variable', 'sans-serif'],
      },
    },
  },
  plugins: [],
}

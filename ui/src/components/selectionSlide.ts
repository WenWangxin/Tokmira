import type { Directive } from 'vue'

type SlideElement = HTMLElement & { __slideCleanup?: () => void; __slideUpdate?: () => void }

export const vSelectionSlide: Directive<SlideElement> = {
  mounted(element) {
    const slider = document.createElement('span')
    slider.className = 'selection-slider'
    slider.setAttribute('aria-hidden', 'true')
    element.prepend(slider)
    let frame = 0
    const update = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => {
        const active = element.querySelector<HTMLElement>('button.active')
        if (!active) { slider.style.opacity = '0'; return }
        slider.style.width = `${active.offsetWidth}px`
        slider.style.height = `${active.offsetHeight}px`
        slider.style.transform = `translate(${active.offsetLeft}px, ${active.offsetTop}px)`
        slider.style.opacity = '1'
        element.classList.add('selection-slide-ready')
      })
    }
    const observer = new ResizeObserver(update)
    observer.observe(element)
    for (const button of element.querySelectorAll('button')) observer.observe(button)
    element.__slideUpdate = update
    element.__slideCleanup = () => { observer.disconnect(); cancelAnimationFrame(frame); slider.remove() }
    update()
  },
  updated(element) { element.__slideUpdate?.() },
  unmounted(element) { element.__slideCleanup?.() },
}

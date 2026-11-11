// The visual corpus of prompt 053: backgrounds, elements, shapes, filters
// and transforms, each a whole theme with no words. `@FIX@` stands for the
// fixtures directory so the cases are the same on every machine.
module.exports = function visuals({ DEFAULT_OVERLAY_THEME, normalizeOverlayTheme, createOverlayElement, OVERLAY_SHAPE_GROUPS }) {
  const FIX = '@FIX@'
  const base = normalizeOverlayTheme(DEFAULT_OVERLAY_THEME)
  const theme = (background, elements = []) => normalizeOverlayTheme({
    ...base,
    background: { ...base.background, ...background },
    elements,
  })
  const cases = []
  const add = (group, id, background, elements) => cases.push({ id: `${group}-${id}`, group, theme: theme(background, elements) })
  const color = { type: 'color', color: '#1b4f72', opacity: 1 }

  add('bg', 'colour', color)
  add('bg', 'colour-opacity', { ...color, opacity: 0.4 })
  add('bg', 'colour-rgba', { ...color, color: 'rgba(200, 30, 30, 0.5)' })
  add('bg', 'transparent', { type: 'transparent' })
  for (const angle of [0, 45, 90, 135, 200, 315]) {
    add('bg', `gradient-${angle}`, { type: 'gradient', color: '#ff2d55', color2: '#2d55ff', angleDeg: angle, opacity: 1 })
  }
  add('bg', 'gradient-alpha', { type: 'gradient', color: 'rgba(255, 255, 0, 1)', color2: 'rgba(0, 0, 255, 0)', angleDeg: 60, opacity: 0.8 })
  add('bg', 'gradient-one-colour', { type: 'gradient', color: '#33aa66', angleDeg: 30, opacity: 1 })
  for (const fit of ['cover', 'contain', 'fill']) {
    add('bg', `png-${fit}`, { type: 'image', mediaPath: `${FIX}/card.png`, mediaFit: fit, opacity: 1 })
  }
  add('bg', 'png-opacity', { type: 'image', mediaPath: `${FIX}/card.png`, mediaFit: 'contain', opacity: 0.5 })
  add('bg', 'png-alpha', { type: 'image', mediaPath: `${FIX}/card-alpha.png`, mediaFit: 'contain', opacity: 1 })
  for (const ext of ['webp', 'bmp', 'avif', 'svg', 'gif']) {
    const file = ext === 'gif' ? 'anim.gif' : `card.${ext}`
    add('bg', `${ext}-contain`, { type: 'image', mediaPath: `${FIX}/${file}`, mediaFit: 'contain', opacity: 1 })
  }
  add('bg', 'svg-cover', { type: 'image', mediaPath: `${FIX}/card.svg`, mediaFit: 'cover', opacity: 1 })
  const picture = { type: 'image', mediaPath: `${FIX}/card.png`, mediaFit: 'contain', opacity: 1 }
  add('filter', 'hue', { ...picture, hue: 90 })
  add('filter', 'hue-negative', { ...picture, hue: -140 })
  add('filter', 'saturate-up', { ...picture, saturation: 1.8 })
  add('filter', 'saturate-down', { ...picture, saturation: 0.2 })
  add('filter', 'brightness-up', { ...picture, brightness: 1.5 })
  add('filter', 'brightness-down', { ...picture, brightness: 0.4 })
  add('filter', 'contrast-up', { ...picture, contrast: 1.6 })
  add('filter', 'contrast-down', { ...picture, contrast: 0.4 })
  add('filter', 'all', { ...picture, hue: 40, saturation: 1.4, brightness: 1.2, contrast: 0.8 })
  add('filter', 'alpha-image', { type: 'image', mediaPath: `${FIX}/card-alpha.png`, mediaFit: 'contain', opacity: 1, hue: 120, saturation: 1.5 })
  for (const blur of [4, 12, 30]) {
    add('blur', `bg-${blur}`, { ...picture, blurPx: blur })
  }
  add('blur', 'bg-cover-12', { ...picture, mediaFit: 'cover', blurPx: 12 })
  add('blur', 'bg-filter-12', { ...picture, blurPx: 12, hue: 90, brightness: 1.3 })

  const el = (kind, id, patch = {}, mediaPath, shape) => ({
    ...createOverlayElement(kind, mediaPath, id, shape),
    ...patch,
  })
  const box = (xPct, yPct, widthPct, heightPct) => ({ xPct, yPct, widthPct, heightPct })
  const none = { type: 'color', color: '#202830', opacity: 1 }
  add('el', 'rect', none, [el('rectangle', 'r', { box: box(10, 10, 30, 30), fill: '#e0a030', fillOpacity: 1 })])
  add('el', 'rect-radius', none, [el('rectangle', 'r', { box: box(10, 10, 30, 30), fill: '#e0a030', radiusPx: 60 })])
  add('el', 'rect-radius-big', none, [el('rectangle', 'r', { box: box(10, 10, 30, 12), fill: '#e0a030', radiusPx: 500 })])
  add('el', 'rect-fill-opacity', none, [el('rectangle', 'r', { box: box(10, 10, 30, 30), fill: '#ff0000', fillOpacity: 0.4 })])
  add('el', 'rect-opacity', none, [el('rectangle', 'r', { box: box(10, 10, 30, 30), fill: '#ff0000', opacity: 0.5 })])
  add('el', 'rect-stroke', none, [el('rectangle', 'r', {
    box: box(10, 10, 30, 30), fill: '#e0a030', radiusPx: 40,
    stroke: { enabled: true, color: '#ffffff', opacity: 0.8, widthPx: 14 },
  })])
  add('el', 'rect-stroke-thick', none, [el('rectangle', 'r', {
    box: box(10, 10, 10, 10), fill: '#e0a030', radiusPx: 20,
    stroke: { enabled: true, color: '#00ffff', opacity: 1, widthPx: 80 },
  })])
  add('el', 'rect-rotated', none, [el('rectangle', 'r', { box: box(30, 30, 40, 20), fill: '#40c080', rotationDeg: 30 })])
  add('el', 'rect-rotated-stroke', none, [el('rectangle', 'r', {
    box: box(30, 30, 40, 20), fill: '#40c080', radiusPx: 30, rotationDeg: -50,
    stroke: { enabled: true, color: '#ffffff', opacity: 1, widthPx: 10 },
  })])
  add('el', 'ellipse', none, [el('ellipse', 'e', { box: box(10, 10, 40, 30), fill: '#a050e0' })])
  add('el', 'ellipse-stroke', none, [el('ellipse', 'e', {
    box: box(10, 10, 40, 30), fill: '#a050e0',
    stroke: { enabled: true, color: '#ffffff', opacity: 1, widthPx: 16 },
  })])
  add('el', 'ellipse-rotated', none, [el('ellipse', 'e', { box: box(20, 25, 50, 20), fill: '#a050e0', rotationDeg: 70 })])
  add('el', 'order', none, [
    el('rectangle', 'a', { box: box(10, 10, 30, 30), fill: '#ff0000' }),
    el('rectangle', 'b', { box: box(25, 25, 30, 30), fill: '#0000ff', fillOpacity: 0.6 }),
    el('ellipse', 'c', { box: box(40, 15, 30, 30), fill: '#00ff00', aboveText: true }),
  ])
  const wanted = ['triangle', 'diamond', 'pentagon', 'hexagon', 'octagon', 'donut', 'pie', 'arch', 'heart', 'lightning', 'sun', 'moon', 'cloud', 'smiley', 'no-symbol',
    'arrow-right', 'arrow-quad', 'arrow-u-turn', 'plus', 'divide', 'star-5', 'star-12', 'burst', 'callout-round-rect', 'callout-cloud',
    'frame', 'flow-document', 'banner-wave', 'line', 'line-arrow', 'line-elbow', 'line-curve', 'line-wave']
  const known = new Set(OVERLAY_SHAPE_GROUPS.flatMap(g => g.shapes.map(s => s.id)))
  for (const id of wanted) {
    if (!known.has(id)) throw new Error(`unknown shape ${id}`)
    add('shape', id, none, [el('shape', 's', { box: box(30, 20, 40, 50), fill: '#f0c040', stroke: { enabled: true, color: '#ffffff', opacity: 1, widthPx: 6 } }, undefined, id)])
  }
  add('shape', 'no-stroke', none, [el('shape', 's', { box: box(30, 20, 40, 50), fill: '#f0c040', stroke: { enabled: false, color: '#fff', opacity: 1, widthPx: 6 } }, undefined, 'heart')])
  add('shape', 'fill-opacity', none, [el('shape', 's', { box: box(30, 20, 40, 50), fill: '#f04040', fillOpacity: 0.5, stroke: { enabled: true, color: '#ffffff', opacity: 0.7, widthPx: 20 } }, undefined, 'star-5')])
  add('shape', 'stretched-wide', none, [el('shape', 's', { box: box(5, 40, 90, 12), fill: '#40c0f0', stroke: { enabled: true, color: '#ffffff', opacity: 1, widthPx: 8 } }, undefined, 'star-5')])
  add('shape', 'rotated', none, [el('shape', 's', { box: box(30, 20, 40, 50), fill: '#f0c040', rotationDeg: 35, stroke: { enabled: true, color: '#ffffff', opacity: 1, widthPx: 8 } }, undefined, 'arrow-right')])
  add('shape', 'flip-x', none, [el('shape', 's', { box: box(30, 20, 40, 50), fill: '#f0c040', flipX: true }, undefined, 'arrow-bent')])
  add('shape', 'flip-y', none, [el('shape', 's', { box: box(30, 20, 40, 50), fill: '#f0c040', flipY: true }, undefined, 'arrow-bent')])
  add('shape', 'flip-both-rotated', none, [el('shape', 's', { box: box(30, 20, 40, 50), fill: '#f0c040', flipX: true, flipY: true, rotationDeg: 30 }, undefined, 'arrow-bent')])
  add('shape', 'stroke-spill', none, [el('shape', 's', { box: box(40, 40, 20, 20), fill: '#f0c040', stroke: { enabled: true, color: '#ff00ff', opacity: 1, widthPx: 60 } }, undefined, 'diamond')])

  const image = (id, patch = {}, file = 'card.png') => el('image', id, { box: box(20, 15, 40, 60), ...patch }, `${FIX}/${file}`)
  for (const fit of ['cover', 'contain', 'fill']) {
    add('media', `image-${fit}`, none, [image('i', { mediaFit: fit })])
  }
  add('media', 'image-rotated', none, [image('i', { mediaFit: 'contain', rotationDeg: 25 })])
  add('media', 'image-cover-rotated', none, [image('i', { mediaFit: 'cover', rotationDeg: -40 })])
  add('media', 'image-flip-x', none, [image('i', { mediaFit: 'contain', flipX: true })])
  add('media', 'image-flip-y', none, [image('i', { mediaFit: 'contain', flipY: true })])
  add('media', 'image-flip-rotated', none, [image('i', { mediaFit: 'contain', flipX: true, rotationDeg: 20 })])
  add('media', 'image-opacity', none, [image('i', { mediaFit: 'contain', opacity: 0.5 })])
  add('media', 'image-filter', none, [image('i', { mediaFit: 'contain', hue: 90, saturation: 1.5, brightness: 1.1, contrast: 1.2 })])
  add('media', 'image-blur', none, [image('i', { mediaFit: 'contain', blurPx: 8 })])
  add('media', 'image-blur-filter', none, [image('i', { mediaFit: 'contain', blurPx: 8, hue: 90, brightness: 1.4 })])
  add('media', 'image-blur-rotated', none, [image('i', { mediaFit: 'contain', blurPx: 6, rotationDeg: 15 })])
  add('media', 'image-svg', none, [image('i', { mediaFit: 'contain' }, 'card.svg')])
  add('media', 'image-webp', none, [image('i', { mediaFit: 'contain' }, 'card.webp')])
  add('media', 'image-alpha', none, [image('i', { mediaFit: 'contain' }, 'card-alpha.png')])
  add('media', 'image-and-shapes', none, [
    el('rectangle', 'under', { box: box(10, 10, 50, 50), fill: '#ffffff', fillOpacity: 0.3 }),
    image('i', { box: box(25, 25, 40, 50), mediaFit: 'cover', opacity: 0.8 }),
    el('shape', 'over', { box: box(50, 20, 30, 40), fill: '#ff3366', aboveText: true }, undefined, 'heart'),
  ])
  add('media', 'missing-file', none, [image('i', {}, 'does-not-exist.png'), el('rectangle', 'r', { box: box(60, 60, 20, 20), fill: '#ff0000' })])
  add('media', 'broken-file', none, [image('i', {}, 'broken.png'), el('rectangle', 'r', { box: box(60, 60, 20, 20), fill: '#ff0000' })])
  return cases
}

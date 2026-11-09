const passages = [
  ['short', 'John 11:35', 'Jesus wept.'],
  ['kjv', 'John 3:16 (KJV)', 'For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.'],
  ['bbe', 'John 3:16 (BBE)', 'For God had such love for the world that he gave his only Son, so that whoever has faith in him may not come to destruction but have eternal life.'],
  ['bsb', 'John 3:16 (BSB)', 'For God so loved the world that He gave His one and only Son, that everyone who believes in Him shall not perish but have eternal life.'],
  ['numbered', 'Psalm 23:1–3 (KJV)', '1 The LORD is my shepherd; I shall not want.\n2 He maketh me to lie down in green pastures: he leadeth me beside the still waters.\n3 He restoreth my soul: he leadeth me in the paths of righteousness for his name’s sake.'],
  ['long', 'Psalm 119 (KJV)', ('Blessed are the undefiled in the way, who walk in the law of the LORD. Blessed are they that keep his testimonies, and that seek him with the whole heart. ').repeat(10).trim()],
  ['yoruba', 'Yorùbá / English', 'Ìjà dópin, ogun sì tán\nThe strife is over, the battle done\nOlúwa wa ti ṣẹ́gun\nOur Lord has won the victory'],
  ['lyrics', '', 'Amazing grace! How sweet the sound\nThat saved a wretch like me\nI once was lost, but now am found\nWas blind, but now I see'],
  ['rtl', 'مزمور ٢٣', 'الرَّبُّ رَاعِيَّ فَلَا يُعْوِزُنِي شَيْءٌ.\nThe Lord is my shepherd — 123.'],
  ['empty', '', ''],
  ['blank-with-reference', 'John 3:16', ' \n  '],
  ['breakword', 'Fixture', '  Faith   hope   and love.\n\n' + 'uninterrupted'.repeat(9) + '\n<light> & “mercy”'],
]
const variants = [
  ['original', {}],
  ['left', { align: 'left', verticalAlign: 'top', letterSpacingPx: 1.5, textTransform: 'uppercase' }],
  ['right', { align: 'right', verticalAlign: 'bottom', letterSpacingPx: -0.4, textTransform: 'lowercase', textDecoration: 'line-through' }],
  ['outline', {
    align: 'justify', textTransform: 'capitalize', textDecoration: 'underline',
    outline: { enabled: true, position: 'outside', widthPx: 2, style: 'solid', color: '#123040', opacity: 0.85 },
  }],
]
module.exports = function corpus(themes) {
  return themes.flatMap(({ id, theme }) => passages.flatMap(([label, reference, text]) =>
    variants.flatMap(([variant, style]) => [false, true].map(autoFit => ({
      id: `${id}-${label}-${variant}-${autoFit ? 'fit' : 'fixed'}`,
      theme: { ...theme, verse: { ...theme.verse, ...style }, layout: { ...theme.layout, autoFitText: autoFit } },
      reference,
      text,
      coloredLines: label === 'yoruba' ? text.split('\n').map((line, i) => ({
        text: line, color: i % 2 ? '#5eead4' : undefined,
      })) : undefined,
    })),
  )))
}

<?php if ($mode === 'header'): ?>
<h1>Header</h1>
<?php elseif ($mode === 'footer'): ?>
<footer>Bye</footer>
<?php else: ?>
<p>Body</p>
<?php endif;

while ($row): echo $row; endwhile;

for ($i = 0; $i < 2; $i++): echo $i; endfor;

foreach ($links as $link): echo $link; endforeach;

declare(ticks=1): echo 'tick'; enddeclare;

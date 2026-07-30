#!/usr/bin/env perl
#
# fcitx5-key-trace.pl — redact and analyse fcitx5 key traces.
#
# Companion to docs/ime-chrome-diagnosis.md and issue #14. The previous round of
# this investigation kept its redaction filter and its detector in a session
# scratchpad; both were lost, and with them every number in the write-up became
# unreproducible. This file exists so that cannot happen again, which is also why
# --selftest pins its fixtures to the log excerpts quoted in the document: if the
# detector and the document ever disagree, CI fails.
#
# Two jobs, deliberately in one file:
#
#   --redact   strip everything from a trace that must never reach disk
#   --report   count trigger-key presses and classify each one
#
# They share @ALLOW_KEYSYM. Splitting them into two programs would let the
# allow-list drift, and a redactor that drops a line the detector needs breaks
# the measurement silently — you would not notice until the numbers looked odd.
#
# WHY --redact IS NOT OPTIONAL
#
#   fcitx5 run with `--verbose key_trace=5` logs every keystroke, and it holds
#   its Wayland keyboard grab while the COSMIC lock screen is up. An unfiltered
#   trace therefore contains the login password in clear. That already happened
#   once during this investigation and had to be scrubbed after the fact.
#
#   There are two channels, not one. The documented recipe only covered the
#   first:
#     1. fcitx5's own `KeyEvent: Key(<keysym>)` lines, plus the rawKey/origKey/
#        keycode fields beside them.
#     2. WAYLAND_DEBUG's `zwp_input_method_keyboard_grab_v2#N.key(serial, time,
#        keycode, state)` — field 3 is a raw evdev keycode and identifies the
#        physical key just as precisely as a keysym does.
#
#   This filter closes both, and fails closed: a line that looks like it carries
#   key material but cannot be decomposed is dropped, never passed through.
#
# Usage:
#   WAYLAND_DEBUG=1 fcitx5 -D --verbose 'default=5,key_trace=5' 2>&1 \
#     | perl scripts/fcitx5-key-trace.pl --redact --stamp >> trace.log
#
#   perl scripts/fcitx5-key-trace.pl --report < trace.log
#   perl scripts/fcitx5-key-trace.pl --report --trials trials.tsv < trace.log
#   perl scripts/fcitx5-key-trace.pl --selftest
#
# Do not rename this file to something extension-less with an sh-family shebang:
# CI's shellcheck action picks those up by shebang and would fail on Perl.

use strict;
use warnings;
use Getopt::Long qw(GetOptions);
use POSIX ();
use Time::HiRes ();

# ---------------------------------------------------------------------------
# Allow-lists, shared by --redact and --report
# ---------------------------------------------------------------------------

# Keysyms whose fcitx5 KeyEvent lines survive redaction. Everything else is
# dropped, so this list is the whole security boundary on channel 1.
#
# Deliberately tighter than the recipe that was in docs/ime-chrome-diagnosis.md:
#   - Shift_L / Shift_R are NOT here. They reveal which positions of a typed
#     secret were capitalised.
#   - BackSpace / Tab are NOT here. BackSpace reveals typing corrections and
#     therefore length information.
#   - Return IS here. A password cannot contain it, and it marks the moment a
#     lock screen was dismissed, which is what correlates a trace with a lock
#     cycle.
our @ALLOW_KEYSYM = (
    'Control+space',        # the trigger key this investigation is about
    'Zenkaku_Hankaku',      # configured trigger, absent from a US layout
    'Hangul',               # configured trigger, absent from a US layout
    'Super+space',
    'Shift+Super+space',
    'Control_L',
    'Control_R',
    'Super_L',
    'Super_R',
    'Return',
);

# Raw evdev keycodes kept in WAYLAND_DEBUG `.key()` lines, and ONLY when
# --keep-trigger-keycodes is passed. The default redacts every keycode.
#
# The default is the safe one because it is what runs during a multi-hour trace
# that will cross a lock screen, and because --report never needs these: it
# works entirely off fcitx5's own KeyEvent lines. Pass the flag only for a short,
# deliberate capture whose purpose is to show that the grab was live and
# delivering at the instant of a failure — the one excerpt an upstream report
# needs.
#
# Residual exposure when the flag IS passed: keycode 57 is space, so a secret
# containing a space would have those positions visible. Low, but real.
our %ALLOW_KEYCODE = (
    29 => 'KEY_LEFTCTRL',
    97 => 'KEY_RIGHTCTRL',
    57 => 'KEY_SPACE',
);

# WAYLAND_DEBUG emits one line per protocol message. Keeping all of it turns a
# multi-hour trace into tens of GB, so the per-frame flood is dropped at write
# time; only the input-method and text-input interfaces are of any interest.
our $WAYLAND_KEEP_RE = qr/
      zwp_input_method_v2
    | zwp_input_method_keyboard_grab_v2
    | zwp_input_method_manager_v2
    | zwp_text_input_v3
    | zwp_text_input_manager_v3
    | wl_keyboard
/x;

# ---------------------------------------------------------------------------
# Line shapes
# ---------------------------------------------------------------------------

# A WAYLAND_DEBUG line, e.g. "[ 906672.653] zwp_input_method_v2#17.activate()".
# libwayland's own stamp is on a clock that matches nothing else on the system,
# which is why --stamp exists.
my $RE_WAYLAND_LINE = qr/^\s*\[\s*\d+\.\d+\]/;

# .key(serial, time, keycode, state) — field 3 is the evdev keycode.
my $RE_WL_KEY = qr/(\.key\(\s*\d+\s*,\s*\d+\s*,\s*)(\d+)(\s*,\s*-?\d+\s*\))/;

# fcitx5's own lines.
my $RE_KEYEVENT = qr/KeyEvent:\s*Key\(/;
my $RE_KEYSYM   = qr/KeyEvent:\s*Key\(([^)\s]+)/;

# A trigger-key press (not the release). Release:0 means "this is a press".
my $RE_PRESS = qr/KeyEvent:\s*Key\(Control\+space[^)]*\)\s+Release:0/;

# Any other configured trigger, counted separately so it never contaminates the
# Control+space ratio that the baseline in docs/ime-chrome-diagnosis.md reports.
my $RE_OTHER_PRESS =
  qr/KeyEvent:\s*Key\((?:Zenkaku_Hankaku|Hangul|Super\+space)[^)]*\)\s+Release:0/;

# The input method actually changed.
my $RE_SWITCH = qr/Input method switched|\[Activating\]:/;

# result:1 = fcitx5 consumed the key; result:0 = it passed through to the client.
my $RE_RESULT = qr/KeyEvent handling time:\s*\d+\s*ms\s+result:(\d)/;

# A focus event, as opposed to the plain Instance::deactivateInputMethod that a
# healthy press also logs. The event_type= suffix is what distinguishes them.
my $RE_FOCUS = qr/deactivateInputMethod\s+event_type=/;

# ---------------------------------------------------------------------------
# Option parsing
# ---------------------------------------------------------------------------

my %opt = (
    'window-ms' => 250,     # a press's effect must land within this to count
    'gap-ms'    => 2000,    # consecutive failures closer than this are one burst
    'defer-ms'  => 2000,    # see classify_press() for why this bound exists
);

GetOptions(
    \%opt,
    'redact',
    'report',
    'selftest',
    'stamp',
    'keep-trigger-keycodes',
    'window-ms=i',
    'gap-ms=i',
    'defer-ms=i',
    'trials=s',
    'json',
    'help',
) or die "bad options; try --help\n";

if ( $opt{help} ) {
    print <<"USAGE";
usage: $0 --redact [--stamp] [--keep-trigger-keycodes]
       $0 --report [--window-ms N] [--gap-ms N] [--defer-ms N]
                   [--trials FILE] [--json]
       $0 --selftest

--redact reads a trace on stdin and writes a version safe to keep on disk.
--report reads a redacted trace on stdin and classifies each trigger-key press.
--selftest runs the built-in fixtures and exits non-zero on any mismatch.

See the comments at the top of this file, and docs/ime-chrome-diagnosis.md.
USAGE
    exit 0;
}

my $mode_count = ( $opt{redact} ? 1 : 0 )
  + ( $opt{report}   ? 1 : 0 )
  + ( $opt{selftest} ? 1 : 0 );
die "pick exactly one of --redact / --report / --selftest\n" if $mode_count != 1;

if    ( $opt{redact} )   { exit run_redact() }
elsif ( $opt{report} )   { exit run_report() }
elsif ( $opt{selftest} ) { exit run_selftest() }

# ---------------------------------------------------------------------------
# --redact
# ---------------------------------------------------------------------------

sub run_redact {
    # Unbuffered: this sits in the middle of a pipeline that may run for hours,
    # and a trace that is still sitting in a buffer when the session dies is a
    # trace that was never taken.
    $| = 1;

    my %allow_sym = map { $_ => 1 } @ALLOW_KEYSYM;

    while ( defined( my $line = <STDIN> ) ) {
        chomp $line;
        my $out = redact_line( $line, \%allow_sym );
        next unless defined $out;
        print $opt{stamp} ? '@' . wallclock() . ' ' . $out . "\n" : $out . "\n";
    }
    return 0;
}

# Returns the redacted line, or undef to drop it entirely.
sub redact_line {
    my ( $line, $allow_sym ) = @_;

    if ( $line =~ $RE_WAYLAND_LINE ) {
        return undef unless $line =~ $WAYLAND_KEEP_RE;

        $line =~ s{$RE_WL_KEY}{
            $1 . ( keycode_kept($2) ? $2 : '<redacted>' ) . $3
        }ge;

        # Fail closed: a .key() we could not decompose might still hold a
        # keycode in a shape this regex does not know.
        if ( $line =~ /\.key\(/
            && $line !~ /\.key\(\s*\d+\s*,\s*\d+\s*,\s*(?:\d+|<redacted>)\s*,\s*-?\d+\s*\)/ )
        {
            return undef;
        }
        return $line;
    }

    if ( $line =~ $RE_KEYEVENT ) {
        my ($sym) = $line =~ $RE_KEYSYM;

        # Fail closed: an unparseable KeyEvent line is dropped, not passed on.
        return undef unless defined $sym;

        # Drop the whole line rather than rewriting the keysym to <redacted>.
        # Rewriting keeps one output line per keystroke, which leaks the length
        # of whatever was typed; dropping leaks nothing. The detector only reads
        # trigger-key lines and the adjacent switch/result lines, so this costs
        # the measurement nothing.
        return undef unless $allow_sym->{$sym};

        unless ( $opt{'keep-trigger-keycodes'} ) {
            $line =~ s/\bkeycode:\s*\d+/keycode: <redacted>/g;
            $line =~ s/\brawKey:\s*\S+/rawKey: <redacted>/g;
            $line =~ s/\borigKey:\s*\S+/origKey: <redacted>/g;
        }
        return $line;
    }

    # Anything else that smells of key material and was not classified above.
    return undef if $line =~ /\.key\(/;

    return $line;
}

sub keycode_kept {
    my ($code) = @_;
    return 0 unless $opt{'keep-trigger-keycodes'};
    return exists $ALLOW_KEYCODE{ 0 + $code } ? 1 : 0;
}

sub wallclock {
    my ( $sec, $usec ) = Time::HiRes::gettimeofday();
    return POSIX::strftime( '%H:%M:%S', localtime($sec) )
      . sprintf( '.%03d', int( $usec / 1000 ) );
}

# ---------------------------------------------------------------------------
# Parsing, shared by --report and --selftest
# ---------------------------------------------------------------------------

# Turns HH:MM:SS.fff into seconds since midnight, as a float.
sub to_seconds {
    my ($ts) = @_;
    my ( $h, $m, $s, $frac ) = $ts =~ /^(\d{2}):(\d{2}):(\d{2})\.(\d+)$/
      or return undef;
    return $h * 3600 + $m * 60 + $s + ( "0.$frac" + 0 );
}

# Each event is { t => float seconds, raw => text }.
#
# A line may carry two timestamps: the one --stamp prefixed (marked with a
# leading '@', which is why that sigil is there) and fcitx5's own. fcitx5's is
# closer to when the event happened, so it wins; the stamp is the fallback for
# WAYLAND_DEBUG lines, whose native clock matches nothing.
sub parse_lines {
    my (@lines) = @_;

    my @ev;
    my $day_offset = 0;
    my $last_t;

    for my $line (@lines) {
        chomp $line;

        my $rest     = $line;
        my $fallback = undef;
        if ( $rest =~ s/^\@(\d{2}:\d{2}:\d{2}\.\d+)\s+// ) {
            $fallback = $1;
        }

        my $ts;
        if ( $rest =~ /(\d{2}:\d{2}:\d{2}\.\d+)/ ) { $ts = $1 }
        $ts = $fallback unless defined $ts;
        next unless defined $ts;

        my $t = to_seconds($ts);
        next unless defined $t;

        # Midnight rollover. Only a real backwards step counts; sub-second
        # jitter between the two clocks must not add a day.
        if ( defined $last_t && $t < $last_t - 1 ) { $day_offset += 86400 }
        $last_t = $t;

        push @ev, { t => $t + $day_offset, raw => $rest };
    }
    return \@ev;
}

# ---------------------------------------------------------------------------
# Classification
# ---------------------------------------------------------------------------

# Outcomes:
#
#   SWITCHED   the input method changed within --window-ms. Working as intended.
#
#   FAILED     nothing happened. `consumed` records result:1 (fcitx5 ate the
#              key) vs result:0 (it passed through). Every one of the 16 baseline
#              failures was result:1; a result:0 failure is a DIFFERENT defect
#              and is reported separately rather than folded in.
#
#   DEFERRED   the switch arrived late, attached to a focus event instead of to
#              the keypress (docs/ime-chrome-diagnosis.md:42-49, where it landed
#              1.4 s later). Its own bucket, because counting it as a success and
#              counting it as a failure are both wrong.
#
# On --defer-ms: distinguishing "the toggle was applied late" from "an unrelated
# focus event cleared the latched state, and a later press did the work" is not
# possible from the log alone — both look like a focus event followed by an
# Activate. A latency bound is the pragmatic separator. 2000 ms puts the 1.4 s
# case in DEFERRED and the 2.7 s recovery of the six-press burst
# (docs/ime-chrome-diagnosis.md:190-198) in FAILED, which is how the document
# reads both. Fixtures 3 and 4 pin exactly this boundary, so changing the default
# breaks CI rather than quietly re-labelling history.
sub classify_press {
    my ( $ev, $i, $next_press_idx ) = @_;

    my $t0        = $ev->[$i]{t};
    my $win_end   = $t0 + $opt{'window-ms'} / 1000;
    my $defer_end = $t0 + $opt{'defer-ms'} / 1000;
    my $limit_t = defined $next_press_idx ? $ev->[$next_press_idx]{t} : undef;

    my $result;
    my $seen_focus = 0;

    for ( my $j = $i + 1 ; $j <= $#{$ev} ; $j++ ) {
        my $e = $ev->[$j];
        last if defined $limit_t && $e->{t} >= $limit_t;
        last if $e->{t} > $defer_end;

        if ( !defined $result && $e->{raw} =~ $RE_RESULT ) { $result = $1 }
        $seen_focus = 1 if $e->{raw} =~ $RE_FOCUS;

        if ( $e->{raw} =~ $RE_SWITCH ) {
            if ( $e->{t} <= $win_end ) {
                return { outcome => 'SWITCHED', t => $t0 };
            }
            if ($seen_focus) {
                return {
                    outcome    => 'DEFERRED',
                    t          => $t0,
                    latency_ms => int( ( $e->{t} - $t0 ) * 1000 + 0.5 ),
                };
            }
        }
    }

    # result: may sit just past the deferral bound in a sparse trace; look for it
    # in the narrow window regardless, since it belongs to this press.
    if ( !defined $result ) {
        for ( my $j = $i + 1 ; $j <= $#{$ev} ; $j++ ) {
            last if $ev->[$j]{t} > $win_end;
            last if defined $limit_t && $ev->[$j]{t} >= $limit_t;
            if ( $ev->[$j]{raw} =~ $RE_RESULT ) { $result = $1; last }
        }
    }

    return {
        outcome  => 'FAILED',
        t        => $t0,
        consumed => ( defined $result && $result eq '1' ) ? 1 : 0,
        result   => $result,
    };
}

sub analyse {
    my ($ev) = @_;

    my @press_idx = grep { $ev->[$_]{raw} =~ $RE_PRESS } 0 .. $#{$ev};
    my $other = grep { $ev->[$_]{raw} =~ $RE_OTHER_PRESS } 0 .. $#{$ev};

    my @presses;
    for my $n ( 0 .. $#press_idx ) {
        my $next = $n < $#press_idx ? $press_idx[ $n + 1 ] : undef;
        push @presses, classify_press( $ev, $press_idx[$n], $next );
    }

    my %c = (
        presses      => scalar @presses,
        switched     => 0,
        failed       => 0,
        deferred     => 0,
        consumed     => 0,
        passthrough  => 0,
        other_trigger_presses => $other,
    );
    for my $p (@presses) {
        $c{ lc $p->{outcome} }++;
        next unless $p->{outcome} eq 'FAILED';
        $p->{consumed} ? $c{consumed}++ : $c{passthrough}++;
    }

    # Bursts: consecutive FAILED presses no more than --gap-ms apart. A burst is
    # one latched episode observed repeatedly, not N independent samples — which
    # is the whole reason the per-press ratio in the baseline must not be read as
    # a probability.
    my @bursts;
    my $cur;
    for my $p (@presses) {
        if ( $p->{outcome} ne 'FAILED' ) { push @bursts, $cur if $cur; $cur = undef; next }
        if ( $cur && $p->{t} - $cur->{last_t} <= $opt{'gap-ms'} / 1000 ) {
            $cur->{size}++;
            $cur->{last_t} = $p->{t};
        }
        else {
            push @bursts, $cur if $cur;
            $cur = { start_t => $p->{t}, last_t => $p->{t}, size => 1 };
        }
    }
    push @bursts, $cur if $cur;

    # What ended each burst: a focus event, or the next press working?
    for my $b (@bursts) {
        $b->{recovery} = 'unknown';
        for my $e ( @{$ev} ) {
            next if $e->{t} <= $b->{last_t};
            if ( $e->{raw} =~ $RE_FOCUS ) { $b->{recovery} = 'focus'; last }
            if ( $e->{raw} =~ $RE_PRESS )  { $b->{recovery} = 'press'; last }
        }
    }

    $c{bursts}      = scalar @bursts;
    $c{burst_sizes} = [ map { $_->{size} } @bursts ];
    $c{ratio}       = $c{presses}
      ? sprintf( '%.1f', 100 * $c{failed} / $c{presses} )
      : '0.0';

    return ( \%c, \@presses, \@bursts );
}

# ---------------------------------------------------------------------------
# Trials
# ---------------------------------------------------------------------------

# A trial is one scripted provocation plus a fixed probe. Its outcome is binary,
# and the presses inside a probe are ONE observation — they are the same latched
# state read several times.
#
# The independence rule is pre-registered here rather than applied by eye: a
# trial whose first press was already inside a burst carried over from the
# previous trial is marked carryover and excluded from the denominator. That is
# checkable from the log, unlike "did the reset click register", which cannot be
# told apart from the focus events the provocation itself generates.
sub load_trials {
    my ($path) = @_;
    open my $fh, '<', $path or die "cannot read $path: $!\n";
    my @t;
    while ( defined( my $line = <$fh> ) ) {
        chomp $line;
        next if $line =~ /^\s*(?:#|$)/;
        my ( $ts, $arm, $prov ) = split /\t/, $line;
        next unless defined $ts;
        my $t = to_seconds($ts);
        next unless defined $t;
        push @t, {
            t    => $t,
            arm  => defined $arm  ? $arm  : '?',
            prov => defined $prov ? $prov : '?',
        };
    }
    close $fh;
    return \@t;
}

sub score_trials {
    my ( $trials, $presses ) = @_;

    for my $n ( 0 .. $#{$trials} ) {
        my $start = $trials->[$n]{t};
        my $end = $n < $#{$trials} ? $trials->[ $n + 1 ]{t} : undef;

        my @mine = grep {
            $_->{t} >= $start && ( !defined $end || $_->{t} < $end )
        } @{$presses};

        $trials->[$n]{presses} = scalar @mine;

        if ( !@mine ) {
            $trials->[$n]{verdict}  = 'NO-PROBE';
            $trials->[$n]{carryover} = 0;
            next;
        }

        # Carried-over burst? Look at the press immediately before this trial's
        # first one.
        my $first = $mine[0];
        my ($prev) = grep { $_->{t} < $first->{t} } reverse @{$presses};
        $trials->[$n]{carryover} =
          ( $prev
              && $prev->{outcome} eq 'FAILED'
              && $first->{t} - $prev->{t} <= $opt{'gap-ms'} / 1000 ) ? 1 : 0;

        $trials->[$n]{verdict} =
          ( grep { $_->{outcome} eq 'FAILED' } @mine ) ? 'FAIL' : 'PASS';
    }
    return $trials;
}

# ---------------------------------------------------------------------------
# --report
# ---------------------------------------------------------------------------

sub run_report {
    my @lines = <STDIN>;
    my $ev = parse_lines(@lines);
    my ( $c, $presses, $bursts ) = analyse($ev);

    my $trials;
    if ( $opt{trials} ) {
        $trials = score_trials( load_trials( $opt{trials} ), $presses );
    }

    if ( $opt{json} ) { print to_json( $c, $bursts, $trials ); return 0 }

    printf "presses=%d switched=%d failed=%d deferred=%d ratio=%s%% "
      . "consumed=%d passthrough=%d bursts=%d burst_sizes=%s\n",
      $c->{presses}, $c->{switched}, $c->{failed}, $c->{deferred},
      $c->{ratio}, $c->{consumed}, $c->{passthrough}, $c->{bursts},
      ( @{ $c->{burst_sizes} } ? join( ',', @{ $c->{burst_sizes} } ) : '-' );

    if ( $c->{other_trigger_presses} ) {
        printf "note: %d press(es) of another configured trigger key, "
          . "counted separately\n", $c->{other_trigger_presses};
    }
    if ( $c->{passthrough} ) {
        printf "note: %d failure(s) had result:0 (passed through, not consumed) "
          . "— a different defect; do not fold into the ratio above\n",
          $c->{passthrough};
    }

    if (@{$bursts}) {
        print "\nbursts:\n";
        for my $b ( @{$bursts} ) {
            printf "  %s  size=%d  recovery=%s  duration=%dms\n",
              hhmmss( $b->{start_t} ), $b->{size}, $b->{recovery},
              int( ( $b->{last_t} - $b->{start_t} ) * 1000 + 0.5 );
        }
    }

    if ($trials) {
        my ( $pass, $fail, $dropped ) = ( 0, 0, 0 );
        print "\ntrials:\n";
        for my $t ( @{$trials} ) {
            my $drop = ( $t->{carryover} || $t->{verdict} eq 'NO-PROBE' );
            $drop ? $dropped++ : ( $t->{verdict} eq 'FAIL' ? $fail++ : $pass++ );
            printf "  %s  arm=%-3s prov=%-10s presses=%d  %s%s\n",
              hhmmss( $t->{t} ), $t->{arm}, $t->{prov}, $t->{presses},
              $t->{verdict}, $drop ? '  (excluded)' : '';
        }
        my $n = $pass + $fail;
        printf "\ntrials=%d trials_failed=%d trials_passed=%d "
          . "trials_excluded=%d trial_fail_rate=%s%%\n",
          $n, $fail, $pass, $dropped,
          $n ? sprintf( '%.0f', 100 * $fail / $n ) : 'n/a';
    }

    return 0;
}

sub hhmmss {
    my ($t) = @_;

    # Not `$t % 86400`: Perl's % truncates its operands to integers, which
    # silently drops the milliseconds this whole tool exists to measure.
    my $x = $t - 86400 * int( $t / 86400 );
    my $h = int( $x / 3600 );
    my $m = int( ( $x - $h * 3600 ) / 60 );
    my $s = $x - $h * 3600 - $m * 60;
    return sprintf( '%02d:%02d:%06.3f', $h, $m, $s );
}

sub to_json {
    my ( $c, $bursts, $trials ) = @_;
    my @parts = (
        qq{"presses":$c->{presses}},
        qq{"switched":$c->{switched}},
        qq{"failed":$c->{failed}},
        qq{"deferred":$c->{deferred}},
        qq{"consumed":$c->{consumed}},
        qq{"passthrough":$c->{passthrough}},
        qq{"bursts":$c->{bursts}},
        qq{"ratio":"$c->{ratio}"},
        '"burst_sizes":[' . join( ',', @{ $c->{burst_sizes} } ) . ']',
    );
    if ($trials) {
        my @rows;
        for my $t ( @{$trials} ) {
            push @rows,
              sprintf(
                '{"arm":"%s","prov":"%s","presses":%d,"verdict":"%s","carryover":%d}',
                $t->{arm}, $t->{prov}, $t->{presses}, $t->{verdict},
                $t->{carryover} );
        }
        push @parts, '"trials":[' . join( ',', @rows ) . ']';
    }
    return '{' . join( ',', @parts ) . "}\n";
}

# ---------------------------------------------------------------------------
# --selftest
# ---------------------------------------------------------------------------

# Every fixture is taken from docs/ime-chrome-diagnosis.md. Fixtures 1, 2 and 5
# use the verbatim log lines quoted there; fixtures 3 and 4 expand the
# document's abbreviated excerpts into the real two-line form (the document
# compresses "KeyEvent ... result:1" onto one line for readability), keeping its
# timestamps exactly. That is the point: if this detector and the document ever
# disagree about what those traces mean, this test fails.

sub run_selftest {
    my $failures = 0;

    # --- 1. healthy press (doc :24-29, verbatim) --------------------------
    $failures += check_report(
        'healthy press switches',
        [
            '16:56:00.156828 KeyEvent: Key(Control+space states=4) Release:0',
            '16:56:00.156892 Instance::deactivateInputMethod',
            '16:56:00.157197 Activate: [Last]: [Activating]:keyboard-us',
            '16:56:00.157264 Input method switched',
            '16:56:00.157661 KeyEvent handling time: 0ms result:1',
        ],
        { presses => 1, switched => 1, failed => 0, deferred => 0 },
    );

    # --- 2. failing press (doc :34-35, verbatim) --------------------------
    $failures += check_report(
        'consumed press with no switch is a failure',
        [
            '16:28:15.043187 KeyEvent: Key(Control+space states=4) Release:0',
            '16:28:15.043215 KeyEvent handling time: 0ms result:1',
        ],
        { presses => 1, switched => 0, failed => 1, consumed => 1 },
    );

    # --- 3. deferred switch (doc :42-49) ---------------------------------
    # The effect arrived 1.412 s late, attached to a focus event.
    $failures += check_report(
        'switch that lands late on a focus event is DEFERRED',
        [
            '16:24:56.745000 KeyEvent: Key(Control+space states=4) Release:0',
            '16:24:56.745200 KeyEvent handling time: 0ms result:1',
            '16:24:58.157000 Instance::deactivateInputMethod event_type=4100',
            '16:24:58.157200 Activate: [Last]: [Activating]:mozc',
        ],
        { presses => 1, switched => 0, failed => 0, deferred => 1 },
    );
    $failures += check_deferred_latency(
        'deferred latency is about 1412 ms',
        [
            '16:24:56.745000 KeyEvent: Key(Control+space states=4) Release:0',
            '16:24:56.745200 KeyEvent handling time: 0ms result:1',
            '16:24:58.157000 Instance::deactivateInputMethod event_type=4100',
            '16:24:58.157200 Activate: [Last]: [Activating]:mozc',
        ],
        1412, 5,
    );

    # --- 4. the six-press burst (doc :190-198) ---------------------------
    # Six consecutive failures, then an unrelated focus event 2.7 s later
    # clears the state, then a press works again. The focus event must NOT
    # turn press six into a DEFERRED success — see classify_press().
    my @burst;
    for my $ts (
        '17:28:30.830000', '17:28:31.940000', '17:28:32.140000',
        '17:28:32.270000', '17:28:32.390000', '17:28:32.780000'
      )
    {
        push @burst, "$ts KeyEvent: Key(Control+space states=4) Release:0";
        my $r = $ts;
        $r =~ s/(\d)$/ $1 + 1 /e;
        push @burst, "$r KeyEvent handling time: 0ms result:1";
    }
    push @burst,
      '17:28:35.520000 Instance::deactivateInputMethod event_type=4100',
      '17:28:35.520500 Activate: [Last]: [Activating]:keyboard-us',
      '17:28:41.410000 KeyEvent: Key(Control+space states=4) Release:0',
      '17:28:41.410400 Activate: [Last]: [Activating]:mozc',
      '17:28:41.410600 Input method switched',
      '17:28:41.410900 KeyEvent handling time: 0ms result:1';

    $failures += check_report(
        'six-press burst counts six failures, not five plus a deferral',
        \@burst,
        {
            presses  => 7,
            switched => 1,
            failed   => 6,
            deferred => 0,
            consumed => 6,
            bursts   => 1,
        },
    );
    $failures += check_burst_shape(
        'burst is one episode of size six, recovered by a focus event',
        \@burst, [6], 'focus',
    );

    # --- 5. two switches inside one wall-clock second (doc :136-140) ------
    # Per-second bucketing cannot see this failure; millisecond resolution can.
    $failures += check_report(
        'two switches in the same second are two presses',
        [
            '16:12:00.100000 KeyEvent: Key(Control+space states=4) Release:0',
            '16:12:00.100500 Input method switched',
            '16:12:00.600000 KeyEvent: Key(Control+space states=4) Release:0',
            '16:12:00.600500 Input method switched',
        ],
        { presses => 2, switched => 2, failed => 0 },
    );

    # --- 6. redaction, channel 1: fcitx5 KeyEvent lines ------------------
    my %allow = map { $_ => 1 } @ALLOW_KEYSYM;
    $failures += check_redact(
        'a non-trigger keystroke is dropped entirely, not rewritten',
        '16:28:15.000000 KeyEvent: Key(a states=0) Release:0 keycode: 38',
        undef, \%allow,
    );
    $failures += check_redact(
        'the trigger key survives',
        '16:28:15.043187 KeyEvent: Key(Control+space states=4) Release:0',
        '16:28:15.043187 KeyEvent: Key(Control+space states=4) Release:0',
        \%allow,
    );
    $failures += check_redact(
        'keycode beside a surviving trigger is redacted by default',
        '16:28:15.043187 KeyEvent: Key(Control+space states=4) Release:0 keycode: 65',
        '16:28:15.043187 KeyEvent: Key(Control+space states=4) Release:0 keycode: <redacted>',
        \%allow,
    );
    $failures += check_redact(
        'an unparseable KeyEvent line fails closed',
        '16:28:15.000000 KeyEvent: Key(',
        undef, \%allow,
    );

    # --- 7. redaction, channel 2: WAYLAND_DEBUG .key() keycodes ----------
    # This is the gap the documented recipe had: field 3 is a raw evdev keycode.
    my $wl_space =
      '[ 906672.653] zwp_input_method_keyboard_grab_v2#21.key(176, 12201550, 57, 1)';
    my $wl_other =
      '[ 906672.653] zwp_input_method_keyboard_grab_v2#21.key(176, 12201550, 40, 1)';

    $failures += check_redact(
        'by default every .key() keycode is redacted, including space',
        $wl_space,
        '[ 906672.653] zwp_input_method_keyboard_grab_v2#21.key(176, 12201550, <redacted>, 1)',
        \%allow,
    );

    {
        local $opt{'keep-trigger-keycodes'} = 1;
        $failures += check_redact(
            'with --keep-trigger-keycodes, space (57) is kept',
            $wl_space, $wl_space, \%allow,
        );
        $failures += check_redact(
            'with --keep-trigger-keycodes, a non-trigger keycode is still redacted',
            $wl_other,
            '[ 906672.653] zwp_input_method_keyboard_grab_v2#21.key(176, 12201550, <redacted>, 1)',
            \%allow,
        );
    }

    $failures += check_redact(
        'per-frame Wayland flood is dropped',
        '[ 906672.700] wl_surface@12.frame(new id wl_callback@33)',
        undef, \%allow,
    );
    $failures += check_redact(
        'input-method protocol traffic is kept',
        '[ 906672.660] zwp_input_method_v2#17.deactivate()',
        '[ 906672.660] zwp_input_method_v2#17.deactivate()',
        \%allow,
    );

    # --- 8. the baseline session's burst shape (issue #14) ---------------
    # The issue lists the wall-clock time of all 16 failures in the baseline
    # session. Grouping them at --gap-ms must reproduce the clusters the issue
    # describes: 5, 7, 2, 1, 1. This is the one fixture built from real measured
    # data rather than from a hand-written trace, so it is what validates the
    # default gap.
    #
    # Note what it also shows: the fourth cluster is the single press at
    # 16:24:56.745, which docs/ime-chrome-diagnosis.md:42-49 identifies as the
    # DEFERRED case. Fed the focus event that arrived 1.4 s later, this detector
    # classifies it as DEFERRED rather than FAILED — so under this tool the
    # baseline reads 15 failed + 1 deferred, not 16 failed. The issue's "16"
    # predates the distinction. Here the failures are fed without that focus
    # event, isolating the grouping.
    my @baseline;
    for my $ts (
        '16:12:15.466', '16:12:15.788', '16:12:15.949', '16:12:16.089',
        '16:12:16.201',
        '16:12:44.210', '16:12:44.451', '16:12:45.025', '16:12:45.149',
        '16:12:45.295', '16:12:45.413', '16:12:46.007',
        '16:13:41.121', '16:13:41.335',
        '16:24:56.745',
        '16:28:15.043',
      )
    {
        push @baseline,
          "$ts KeyEvent: Key(Control+space states=4) Release:0",
          "$ts KeyEvent handling time: 0ms result:1";
    }
    # Recovery attribution is not checked here: this synthetic trace contains
    # nothing but the failures, so there are no intervening focus events or
    # successful presses for it to attribute to. The grouping is the claim.
    $failures += check_burst_shape(
        "baseline session's 16 failures group into 5,7,2,1,1 clusters",
        \@baseline, [ 5, 7, 2, 1, 1 ], undef,
    );
    $failures += check_report(
        'baseline failures are all consumed (result:1)',
        \@baseline,
        { presses => 16, switched => 0, failed => 16, consumed => 16, passthrough => 0 },
    );

    # --- 9. --stamp round trip -------------------------------------------
    # A stamped WAYLAND_DEBUG line must still be timestamped for the detector,
    # since libwayland's own clock matches nothing on the system.
    $failures += check_report(
        'stamped lines are parsed via the @-prefixed wall clock',
        [
            '@16:28:15.043 [ 906672.653] zwp_input_method_v2#17.activate()',
            '16:28:15.043187 KeyEvent: Key(Control+space states=4) Release:0',
            '16:28:15.043215 KeyEvent handling time: 0ms result:1',
        ],
        { presses => 1, failed => 1, consumed => 1 },
    );

    # --- 10. the pre-registered independence rule ------------------------
    # A trial whose first press was already inside a burst carried over from the
    # previous trial is not an independent observation and must be excluded. This
    # is the rule the experiment is pre-registered on, so it gets a test: it must
    # not drift into a post-hoc judgement call.
    {
        my @lines = (
            # trial 1: fails
            '11:00:01.000000 KeyEvent: Key(Control+space states=4) Release:0',
            '11:00:01.000200 KeyEvent handling time: 0ms result:1',
            # trial 2 starts 300 ms later — still inside the same latched state
            '11:00:01.300000 KeyEvent: Key(Control+space states=4) Release:0',
            '11:00:01.300200 KeyEvent handling time: 0ms result:1',
            # trial 3 starts well clear of it
            '11:00:30.000000 KeyEvent: Key(Control+space states=4) Release:0',
            '11:00:30.000200 KeyEvent handling time: 0ms result:1',
        );
        my ( undef, $presses ) = analyse( parse_lines(@lines) );
        my $trials = score_trials(
            [
                { t => to_seconds('11:00:00.900'), arm => 'A', prov => 'p' },
                { t => to_seconds('11:00:01.200'), arm => 'A', prov => 'p' },
                { t => to_seconds('11:00:29.900'), arm => 'A', prov => 'p' },
            ],
            $presses,
        );
        my $got = join ',', map { $_->{carryover} } @{$trials};
        $failures += report_check(
            'a trial starting inside the previous burst is excluded as carryover',
            $got eq '0,1,0' ? [] : ["want carryover 0,1,0 got $got"],
        );
    }

    # --- 11. timestamp formatting ----------------------------------------
    # Display-only, but the trial table is what feeds the write-up, so a
    # timestamp that silently loses its milliseconds is actively misleading.
    for my $case ( [ 36000.9, '10:00:00.900' ], [ 62915.466, '17:28:35.466' ] ) {
        my $got = hhmmss( $case->[0] );
        $failures += report_check(
            "hhmmss($case->[0]) keeps its milliseconds",
            $got eq $case->[1] ? [] : ["want $case->[1], got $got"],
        );
    }

    if ($failures) {
        printf STDERR "\n%d selftest check(s) FAILED\n", $failures;
        return 1;
    }
    print "all selftest checks passed\n";
    return 0;
}

sub check_report {
    my ( $name, $lines, $want ) = @_;
    my ($c) = analyse( parse_lines( @{$lines} ) );
    my @bad;
    for my $k ( sort keys %{$want} ) {
        my $got = $k eq 'bursts' ? $c->{bursts} : $c->{$k};
        push @bad, "$k: want $want->{$k}, got " . ( defined $got ? $got : 'undef' )
          if !defined $got || $got != $want->{$k};
    }
    return report_check( $name, \@bad );
}

sub check_deferred_latency {
    my ( $name, $lines, $want, $tol ) = @_;
    my ( undef, $presses ) = analyse( parse_lines( @{$lines} ) );
    my @def = grep { $_->{outcome} eq 'DEFERRED' } @{$presses};
    my @bad;
    if ( @def != 1 ) { push @bad, 'expected exactly one DEFERRED press' }
    elsif ( abs( $def[0]{latency_ms} - $want ) > $tol ) {
        push @bad, "latency: want ~${want}ms, got $def[0]{latency_ms}ms";
    }
    return report_check( $name, \@bad );
}

sub check_burst_shape {
    my ( $name, $lines, $want_sizes, $want_recovery ) = @_;
    my ( undef, undef, $bursts ) = analyse( parse_lines( @{$lines} ) );
    my @bad;
    my $got = join ',', map { $_->{size} } @{$bursts};
    my $exp = join ',', @{$want_sizes};
    push @bad, "burst sizes: want [$exp], got [$got]" if $got ne $exp;
    if (   defined $want_recovery
        && @{$bursts}
        && $bursts->[0]{recovery} ne $want_recovery )
    {
        push @bad, "recovery: want $want_recovery, got $bursts->[0]{recovery}";
    }
    return report_check( $name, \@bad );
}

sub check_redact {
    my ( $name, $in, $want, $allow ) = @_;
    my $got = redact_line( $in, $allow );
    my @bad;
    if ( !defined $want && defined $got ) {
        push @bad, "expected the line to be dropped, got: $got";
    }
    elsif ( defined $want && !defined $got ) {
        push @bad, 'expected the line to survive, it was dropped';
    }
    elsif ( defined $want && $got ne $want ) {
        push @bad, "want: $want\n       got:  $got";
    }
    return report_check( $name, \@bad );
}

sub report_check {
    my ( $name, $bad ) = @_;
    if ( @{$bad} ) {
        print "FAIL $name\n";
        print "     $_\n" for @{$bad};
        return 1;
    }
    print "ok   $name\n";
    return 0;
}

package org.freetak.atn;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.StateListDrawable;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.TextView;

/**
 * REM's "Tactical Blue" design tokens (REM design.md) for the plugin's native screen: dark surface,
 * gradient instrument panels with blue borders, ice-blue accents and pressable gradient buttons.
 * REM loads Rajdhani / Chakra Petch from the web; the plugin uses the closest system faces.
 */
final class RemStyle {
    static final int SURFACE = 0xFF020710;
    static final int PANEL_TOP = 0xFF091937;
    static final int PANEL_BOTTOM = 0xFF071025;
    static final int BORDER = 0x734A85CF;
    static final int PRIMARY = 0xFF64BEFF;
    static final int SECONDARY = 0xFF9CB3D6;
    static final int ON_SURFACE = 0xFFDEF1FF;
    static final int ERROR = 0xFFFFB4AB;
    private static final int BUTTON_TEXT = 0xFF8FDBFF;
    private static final int BUTTON_TEXT_PRESSED = 0xFF042541;
    private static final int INPUT = 0xFF06152D;
    private static final int INPUT_BORDER = 0x665091CD;
    /** Rajdhani-like: condensed, bold, used uppercase for headings and controls. */
    static final Typeface HEADLINE = Typeface.create("sans-serif-condensed", Typeface.BOLD);
    /** Chakra Petch stand-in for operational body text. */
    static final Typeface BODY = Typeface.create("sans-serif", Typeface.NORMAL);

    private RemStyle() { }

    static int dp(Context context, float value) {
        return Math.round(value * context.getResources().getDisplayMetrics().density);
    }

    private static GradientDrawable gradient(Context context, int top, int bottom, int stroke, float radius) {
        GradientDrawable shape = new GradientDrawable(GradientDrawable.Orientation.TOP_BOTTOM, new int[] {top, bottom});
        shape.setCornerRadius(dp(context, radius));
        shape.setStroke(dp(context, 1), stroke);
        return shape;
    }

    /** A rounded tactical card. */
    static Drawable panel(Context context) {
        return gradient(context, PANEL_TOP, PANEL_BOTTOM, BORDER, 14);
    }

    /** REM's shared button: gradient fill, inverting to ice blue while pressed. */
    static void button(Button button, boolean danger) {
        Context context = button.getContext();
        StateListDrawable states = new StateListDrawable();
        states.addState(new int[] {android.R.attr.state_pressed},
            gradient(context, 0xF5C4F0FF, 0xF076D4FF, 0xDBE0F8FF, 10));
        states.addState(new int[] {},
            gradient(context, 0xE00A2348, 0xEB061836, danger ? 0x80FFB4AB : BORDER, 10));
        button.setBackground(states);
        button.setStateListAnimator(null);
        button.setTextColor(new ColorStateList(
            new int[][] {{android.R.attr.state_pressed}, {}},
            new int[] {BUTTON_TEXT_PRESSED, danger ? ERROR : BUTTON_TEXT}));
        button.setTypeface(HEADLINE);
        button.setAllCaps(true);
        button.setLetterSpacing(0.08f);
        button.setTextSize(15);
        button.setMinHeight(dp(context, 48));
        int horizontal = dp(context, 14);
        button.setPadding(horizontal, 0, horizontal, 0);
    }

    static void heading(TextView text) {
        text.setTypeface(HEADLINE);
        text.setAllCaps(true);
        text.setLetterSpacing(0.12f);
        text.setTextColor(PRIMARY);
        text.setTextSize(15);
    }

    static void title(TextView text) {
        text.setTypeface(HEADLINE);
        text.setTextColor(ON_SURFACE);
        text.setTextSize(26);
    }

    static void body(TextView text, boolean secondary) {
        text.setTypeface(BODY);
        text.setTextColor(secondary ? SECONDARY : ON_SURFACE);
    }

    static void input(EditText editor) {
        Context context = editor.getContext();
        GradientDrawable shape = new GradientDrawable();
        shape.setColor(INPUT);
        shape.setCornerRadius(dp(context, 8));
        shape.setStroke(dp(context, 1), INPUT_BORDER);
        editor.setBackground(shape);
        editor.setTextColor(0xFFE0F2FE);
        editor.setHintTextColor(SECONDARY);
        editor.setTypeface(Typeface.MONOSPACE);
        editor.setTextSize(13);
        int padding = dp(context, 10);
        editor.setPadding(padding, padding, padding, padding);
    }

    static void checkbox(CheckBox box) {
        box.setButtonTintList(ColorStateList.valueOf(PRIMARY));
        box.setTextColor(ON_SURFACE);
        box.setTypeface(BODY);
        box.setTextSize(15);
    }
}

package jp.perfectviewer.rs

import androidx.compose.material3.LocalTextStyle
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit
import java.util.Locale

/** 画面の言語。`code` は設定に保存する値 */
enum class Lang(val code: String, val nativeName: String) {
    JA("ja", "日本語"),
    EN("en", "English"),
    ZH("zh", "中文（简体）"),
    KO("ko", "한국어"),
    ES("es", "Español"),
}

/** 設定で選べる言語の値。空文字は端末の言語に従う */
const val LANGUAGE_AUTO = ""

/**
 * 画面の文言の翻訳。コード中の文言は日本語で書き、表示する直前に `t` で選んだ言語へ引き直す。
 * 言語は Compose の状態なので、変えると表示がすぐ入れ替わる。
 */
object Tr {
    var lang by mutableStateOf(Lang.JA)
        private set

    /** 設定値（"" / "ja" / "en" / "zh" / "ko" / "es"）から、使う言語を決める。端末の言語が未対応なら英語 */
    fun resolve(setting: String): Lang =
        Lang.entries.firstOrNull { it.code == setting }
            ?: Lang.entries.firstOrNull { it.code == Locale.getDefault().language }
            ?: Lang.EN

    fun apply(setting: String) {
        lang = resolve(setting)
    }

    /** 翻訳する。表に無い文言や日本語のときは、そのまま返す */
    fun t(ja: String): String {
        if (lang == Lang.JA) return ja
        return TRANSLATIONS[ja]?.getOrNull(lang.ordinal - 1) ?: ja
    }

    /** 翻訳したうえで `{1}` `{2}` に値を差し込む */
    fun f(ja: String, vararg args: Any?): String {
        var s = t(ja)
        args.forEachIndexed { i, a -> s = s.replace("{${i + 1}}", a.toString()) }
        return s
    }
}

/** 翻訳つきの `Text`。同じパッケージの `Text` は、material3 の `Text` を import しない限りこちらが使われる */
@Composable
fun Text(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = Color.Unspecified,
    fontSize: TextUnit = TextUnit.Unspecified,
    fontWeight: FontWeight? = null,
    textAlign: TextAlign? = null,
    overflow: TextOverflow = TextOverflow.Clip,
    maxLines: Int = Int.MAX_VALUE,
    style: TextStyle = LocalTextStyle.current,
) {
    androidx.compose.material3.Text(
        text = Tr.t(text),
        modifier = modifier,
        color = color,
        fontSize = fontSize,
        fontWeight = fontWeight,
        textAlign = textAlign,
        overflow = overflow,
        maxLines = maxLines,
        style = style,
    )
}
